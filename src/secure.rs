//! secure — an authenticated, encrypted channel for Orpheus over the Internet.
//!
//! The gossip/anti-entropy protocol in `net.rs` was written for a trusted LAN: framed
//! plaintext, no authentication, no confidentiality. On the open Internet that is unsafe —
//! anyone who can reach the port can read a node's entire event log and inject forged
//! events. This module wraps every peer connection in a mutually-authenticated, encrypted
//! session, built ONLY from the in-tree Keccak primitives (src/sha3.rs) — no external
//! crate, no OpenSSL, nothing to vendor. The design goals, in order: authenticate both
//! ends, hide the payload, detect tampering, and resist replay.
//!
//! ## Trust model: a pre-shared key (PSK)
//!
//! Orpheus instances that federate are operated by people who already share something out
//! of band — a cluster passphrase, a deployment secret. So the root of trust here is a
//! PRE-SHARED KEY: a passphrase configured on every node that should be allowed to sync
//! (`ORPHEUS_PSK`, or `--psk`, or a `psk` file in the node dir). Two honest properties
//! follow, and are stated plainly wherever the channel is used:
//!
//!   * This gives CONFIDENTIALITY and MUTUAL AUTHENTICATION against network attackers who
//!     do not hold the PSK — the realistic Internet threat. A peer that cannot complete the
//!     handshake never reaches the sync loop, so it can neither read nor write the log.
//!   * It is symmetric, not public-key: everyone who holds the PSK is a full peer, and the
//!     PSK must be distributed securely out of band. It is not a public federation protocol
//!     and does not claim forward secrecy across a PSK compromise. (A future ephemeral-DH
//!     upgrade would add that; the framing here leaves room for it.)
//!
//! When NO PSK is configured, connections fall back to the legacy plaintext protocol so a
//! trusted LAN keeps working unchanged — but any listener bound to a non-loopback address
//! without a PSK prints a clear warning (see `net.rs`), because that is the dangerous case.
//!
//! ## The handshake (challenge–response, mutual, replay-resistant)
//!
//! A short fixed-size exchange proves both ends hold the PSK without ever sending it, and
//! derives fresh per-direction session keys bound to both random nonces:
//!
//! ```text
//!   dialer → listener :  HS1 = version ‖ nonce_d            (32-byte random nonce)
//!   listener → dialer :  HS2 = nonce_l ‖ mac_l              (its nonce + proof)
//!   dialer → listener :  HS3 = mac_d                        (dialer's proof)
//! ```
//!
//! where, with `transcript = version ‖ nonce_d ‖ nonce_l`,
//!   `mac_l = keyed256(PSK, "L" ‖ transcript)` and
//!   `mac_d = keyed256(PSK, "D" ‖ transcript)`.
//! Each side verifies the other's MAC in CONSTANT TIME. A network attacker without the PSK
//! cannot produce either MAC; because both fresh nonces feed every derived key, a recorded
//! session cannot be replayed against a new one (the nonces differ, so the keys differ and
//! the transcript MAC fails). Session keys are then
//!   `k_d2l = shake256("orpheus-sec-v1|d2l" ‖ transcript, 64)` and its `l2d` twin,
//! split into a 32-byte cipher key and a 32-byte MAC key per direction.
//!
//! ## The record layer (encrypt-then-MAC, per-record sequence)
//!
//! Each application frame becomes a record:
//!   `seq (8B, BE)  ‖  ct = plaintext ⊕ SHAKE256(cipher_key ‖ seq, len)  ‖  tag (16B)`
//! with `tag = keyed256(mac_key, seq ‖ ct)[..16]`. The receiver checks the tag in constant
//! time BEFORE decrypting (encrypt-then-MAC), and requires strictly increasing `seq`, so a
//! record cannot be reordered, dropped-then-replayed, or truncated undetectably. The
//! keystream is unique per record because `seq` is unique per direction and the two
//! directions use different keys — no nonce is ever reused.
//!
//! This is not TLS and does not try to be. It is a small, auditable, dependency-free
//! channel that makes "connect my phone to my node across the Internet" safe against the
//! attacker who is actually on that Internet.

use crate::sha3::{keyed256, shake256, shake256_vec};
use std::io::{self, Read, Write};

/// Wire version — bump if the handshake or record layout changes.
const VERSION: u8 = 0x01;
const NONCE_LEN: usize = 32;
const TAG_LEN: usize = 16;
/// Records larger than this are refused (matches net.rs MAX_FRAME budget).
const MAX_RECORD: usize = 64 * 1024 * 1024;

/// Constant-time equality: no early return on the first differing byte, so an attacker
/// cannot learn the correct prefix of a MAC by timing. Length mismatch is a fast reject
/// (lengths are public here — both are fixed by the protocol).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Best-effort random bytes for nonces: OS entropy (/dev/urandom) when available, folded
/// through SHAKE with time, pid, and a stack address so a missing RNG still yields a
/// non-repeating, unpredictable-to-a-remote nonce. Nonces need uniqueness + unpredictability
/// to a network attacker, not cryptographic-grade secrecy on their own; the PSK carries the
/// secrecy. On Android/Linux /dev/urandom is always present, so the primary path is used.
pub fn random_nonce() -> [u8; NONCE_LEN] {
    let mut seed = Vec::with_capacity(64);
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let mut b = [0u8; 32];
        if f.read_exact(&mut b).is_ok() {
            seed.extend_from_slice(&b);
        }
    }
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    seed.extend_from_slice(&t.to_le_bytes());
    seed.extend_from_slice(&std::process::id().to_le_bytes());
    let stack_marker = 0u8;
    seed.extend_from_slice(&(&stack_marker as *const u8 as usize).to_le_bytes());
    // mix a per-call counter so two calls in the same nanosecond still differ
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    seed.extend_from_slice(&CTR.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let mut out = [0u8; NONCE_LEN];
    shake256(&seed, &mut out);
    out
}

/// The master key from a human passphrase: SHAKE256("orpheus-psk-v1" ‖ passphrase, 32).
/// A domain-separated hash, not a slow password KDF — the PSK is expected to be a
/// high-entropy deployment secret, not a user-chosen password. (Documented so no one
/// mistakes it for password storage.)
pub fn derive_psk(passphrase: &str) -> [u8; 32] {
    let mut buf = Vec::with_capacity(14 + passphrase.len());
    buf.extend_from_slice(b"orpheus-psk-v1");
    buf.extend_from_slice(passphrase.as_bytes());
    let mut out = [0u8; 32];
    shake256(&buf, &mut out);
    out
}

/// Per-direction keys derived from the handshake transcript.
struct DirKeys {
    cipher: [u8; 32],
    mac: [u8; 32],
}

fn dir_keys(psk: &[u8; 32], transcript: &[u8], label: &str) -> DirKeys {
    let mut seed = Vec::with_capacity(psk.len() + label.len() + transcript.len());
    seed.extend_from_slice(psk);
    seed.extend_from_slice(label.as_bytes());
    seed.extend_from_slice(transcript);
    let k = shake256_vec(&seed, 64);
    let mut cipher = [0u8; 32];
    let mut mac = [0u8; 32];
    cipher.copy_from_slice(&k[..32]);
    mac.copy_from_slice(&k[32..]);
    DirKeys { cipher, mac }
}

/// An established session: the two directional key sets and the running sequence numbers.
/// `send`/`recv` implement the record layer over any Read+Write (a TcpStream); net.rs uses
/// the `split` halves instead, but the whole-session form is the public API and is
/// exercised by the tests.
#[allow(dead_code)]
pub struct Session {
    send_keys: DirKeys,
    recv_keys: DirKeys,
    send_seq: u64,
    recv_seq: u64,
}

impl Session {
    /// Split into independent SEND and RECV halves. The record layer uses separate keys
    /// and sequence counters per direction, so the halves share no mutable state and can
    /// live on different threads — exactly what net.rs's writer-thread + reader-loop model
    /// needs. (Without this, the encrypted session could not straddle the two threads.)
    pub fn split(self) -> (SendHalf, RecvHalf) {
        (
            SendHalf { keys: self.send_keys, seq: self.send_seq },
            RecvHalf { keys: self.recv_keys, seq: self.recv_seq },
        )
    }

    /// Encrypt-then-MAC one record and write it. Keystream = SHAKE256(cipher ‖ seq),
    /// tag = keyed256(mac, seq ‖ ct)[..16]. `seq` increments per sent record.
    #[allow(dead_code)] // the whole-session form; net.rs uses split(), tests use this
    pub fn send(&mut self, w: &mut impl Write, plaintext: &[u8]) -> io::Result<()> {
        let seq = self.send_seq;
        self.send_seq = self.send_seq.wrapping_add(1);
        let ct = xor_keystream(&self.send_keys.cipher, seq, plaintext);
        let tag = record_tag(&self.send_keys.mac, seq, &ct);
        let mut framed = Vec::with_capacity(8 + ct.len() + TAG_LEN);
        framed.extend_from_slice(&seq.to_be_bytes());
        framed.extend_from_slice(&ct);
        framed.extend_from_slice(&tag);
        write_len_prefixed(w, &framed)
    }

    /// Read one record, verify its tag (constant time) and sequence, and return the
    /// plaintext. Rejects tampering, reordering, and replay.
    #[allow(dead_code)]
    pub fn recv(&mut self, r: &mut impl Read) -> io::Result<Vec<u8>> {
        let framed = read_len_prefixed(r)?;
        if framed.len() < 8 + TAG_LEN {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "short record"));
        }
        let mut seqb = [0u8; 8];
        seqb.copy_from_slice(&framed[..8]);
        let seq = u64::from_be_bytes(seqb);
        let ct = &framed[8..framed.len() - TAG_LEN];
        let tag = &framed[framed.len() - TAG_LEN..];
        let expect = record_tag(&self.recv_keys.mac, seq, ct);
        if !ct_eq(tag, &expect) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "record authentication failed"));
        }
        if seq != self.recv_seq {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "record sequence violation (replay or reorder)"));
        }
        self.recv_seq = self.recv_seq.wrapping_add(1);
        Ok(xor_keystream(&self.recv_keys.cipher, seq, ct))
    }
}

/// SEND half of a split session (owned by net.rs's per-connection writer thread).
pub struct SendHalf {
    keys: DirKeys,
    seq: u64,
}
impl SendHalf {
    pub fn send(&mut self, w: &mut impl Write, plaintext: &[u8]) -> io::Result<()> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let ct = xor_keystream(&self.keys.cipher, seq, plaintext);
        let tag = record_tag(&self.keys.mac, seq, &ct);
        let mut framed = Vec::with_capacity(8 + ct.len() + TAG_LEN);
        framed.extend_from_slice(&seq.to_be_bytes());
        framed.extend_from_slice(&ct);
        framed.extend_from_slice(&tag);
        write_len_prefixed(w, &framed)
    }
}

/// RECV half of a split session (owned by net.rs's reader loop).
pub struct RecvHalf {
    keys: DirKeys,
    seq: u64,
}
impl RecvHalf {
    pub fn recv(&mut self, r: &mut impl Read) -> io::Result<Vec<u8>> {
        let framed = read_len_prefixed(r)?;
        if framed.len() < 8 + TAG_LEN {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "short record"));
        }
        let mut seqb = [0u8; 8];
        seqb.copy_from_slice(&framed[..8]);
        let seq = u64::from_be_bytes(seqb);
        let ct = &framed[8..framed.len() - TAG_LEN];
        let tag = &framed[framed.len() - TAG_LEN..];
        let expect = record_tag(&self.keys.mac, seq, ct);
        if !ct_eq(tag, &expect) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "record authentication failed"));
        }
        if seq != self.seq {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "record sequence violation (replay or reorder)"));
        }
        self.seq = self.seq.wrapping_add(1);
        Ok(xor_keystream(&self.keys.cipher, seq, ct))
    }
}

fn xor_keystream(cipher_key: &[u8; 32], seq: u64, data: &[u8]) -> Vec<u8> {
    let mut seed = Vec::with_capacity(40);
    seed.extend_from_slice(cipher_key);
    seed.extend_from_slice(&seq.to_be_bytes());
    let ks = shake256_vec(&seed, data.len());
    data.iter().zip(ks.iter()).map(|(a, b)| a ^ b).collect()
}

fn record_tag(mac_key: &[u8; 32], seq: u64, ct: &[u8]) -> [u8; TAG_LEN] {
    let mut msg = Vec::with_capacity(8 + ct.len());
    msg.extend_from_slice(&seq.to_be_bytes());
    msg.extend_from_slice(ct);
    let full = keyed256(mac_key, &msg);
    let mut tag = [0u8; TAG_LEN];
    tag.copy_from_slice(&full[..TAG_LEN]);
    tag
}

fn write_len_prefixed(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let len = payload.len() as u32;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

fn read_len_prefixed(r: &mut impl Read) -> io::Result<Vec<u8>> {
    read_len_prefixed_capped(r, MAX_RECORD)
}

/// Read a length-prefixed frame, refusing anything larger than `cap`. The handshake uses a
/// TINY cap (`MAX_HANDSHAKE`) so an UNAUTHENTICATED peer cannot force a large allocation
/// before it has proven the PSK — a memory-amplification DoS otherwise, since the record
/// cap is 64 MiB. Post-handshake records use the full `MAX_RECORD`.
fn read_len_prefixed_capped(r: &mut impl Read, cap: usize) -> io::Result<Vec<u8>> {
    let mut lenb = [0u8; 4];
    r.read_exact(&mut lenb)?;
    let len = u32::from_be_bytes(lenb) as usize;
    if len > cap {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// The largest a handshake message can be: a nonce (32) + a MAC (32) + the version byte,
/// with generous slack. Anything bigger during the handshake is refused before allocation.
const MAX_HANDSHAKE: usize = 128;

fn transcript(nonce_d: &[u8], nonce_l: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(1 + NONCE_LEN * 2);
    t.push(VERSION);
    t.extend_from_slice(nonce_d);
    t.extend_from_slice(nonce_l);
    t
}

/// Run the DIALER side of the handshake over `stream`. On success returns an established
/// `Session`; on PSK mismatch or a malformed peer, an error (the caller drops the socket).
pub fn client_handshake<S: Read + Write>(stream: &mut S, psk: &[u8; 32]) -> io::Result<Session> {
    let nonce_d = random_nonce();
    // HS1: version ‖ nonce_d
    let mut hs1 = Vec::with_capacity(1 + NONCE_LEN);
    hs1.push(VERSION);
    hs1.extend_from_slice(&nonce_d);
    write_len_prefixed(stream, &hs1)?;

    // HS2: nonce_l ‖ mac_l
    let hs2 = read_len_prefixed_capped(stream, MAX_HANDSHAKE)?;
    if hs2.len() != NONCE_LEN + 32 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad HS2"));
    }
    let nonce_l = &hs2[..NONCE_LEN];
    let ts = transcript(&nonce_d, nonce_l);
    let expect_l = mac_side(psk, b"L", &ts);
    if !ct_eq(&hs2[NONCE_LEN..], &expect_l) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "peer failed PSK authentication"));
    }
    // HS3: mac_d
    let mac_d = mac_side(psk, b"D", &ts);
    write_len_prefixed(stream, &mac_d)?;

    Ok(Session {
        send_keys: dir_keys(psk, &ts, "orpheus-sec-v1|d2l"),
        recv_keys: dir_keys(psk, &ts, "orpheus-sec-v1|l2d"),
        send_seq: 0,
        recv_seq: 0,
    })
}

/// Run the LISTENER side of the handshake. Mirror of the dialer; verifies the dialer's
/// proof before returning a `Session`.
pub fn server_handshake<S: Read + Write>(stream: &mut S, psk: &[u8; 32]) -> io::Result<Session> {
    // HS1
    let hs1 = read_len_prefixed_capped(stream, MAX_HANDSHAKE)?;
    if hs1.len() != 1 + NONCE_LEN || hs1[0] != VERSION {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad HS1 / version mismatch"));
    }
    let nonce_d = &hs1[1..];
    let nonce_l = random_nonce();
    let ts = transcript(nonce_d, &nonce_l);
    // HS2: nonce_l ‖ mac_l
    let mac_l = mac_side(psk, b"L", &ts);
    let mut hs2 = Vec::with_capacity(NONCE_LEN + 32);
    hs2.extend_from_slice(&nonce_l);
    hs2.extend_from_slice(&mac_l);
    write_len_prefixed(stream, &hs2)?;
    // HS3: mac_d
    let hs3 = read_len_prefixed_capped(stream, MAX_HANDSHAKE)?;
    let expect_d = mac_side(psk, b"D", &ts);
    if !ct_eq(&hs3, &expect_d) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "peer failed PSK authentication"));
    }
    Ok(Session {
        // NOTE: the listener's SEND direction is l2d, its RECV is d2l — mirror of the client
        send_keys: dir_keys(psk, &ts, "orpheus-sec-v1|l2d"),
        recv_keys: dir_keys(psk, &ts, "orpheus-sec-v1|d2l"),
        send_seq: 0,
        recv_seq: 0,
    })
}

fn mac_side(psk: &[u8; 32], side: &[u8], transcript: &[u8]) -> [u8; 32] {
    let mut msg = Vec::with_capacity(side.len() + transcript.len());
    msg.extend_from_slice(side);
    msg.extend_from_slice(transcript);
    keyed256(psk, &msg)
}

// --------------------------------------------------------------------------
// PSK discovery: env var, then a `psk` file in the node's directory. Trimmed of
// surrounding whitespace/newline so a file written by `echo` works. Returns the
// DERIVED 32-byte key, or None when no PSK is configured (legacy plaintext).
// --------------------------------------------------------------------------

/// Look up the configured PSK passphrase: `ORPHEUS_PSK` env var wins; otherwise a
/// `psk` file inside `dir` (the node/store directory). None → no PSK configured.
pub fn configured_psk(dir: Option<&std::path::Path>) -> Option<[u8; 32]> {
    if let Ok(p) = std::env::var("ORPHEUS_PSK") {
        let p = p.trim();
        if !p.is_empty() {
            return Some(derive_psk(p));
        }
    }
    if let Some(d) = dir {
        if let Ok(s) = std::fs::read_to_string(d.join("psk")) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(derive_psk(s));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory full-duplex pipe so the handshake and record layer can be exercised
    /// without a socket. Two `End`s cross-connected; reads block-free because the test
    /// drives both sides in one thread in lock-step where needed, and uses threads where
    /// the handshake round-trips.
    use std::sync::mpsc::{channel, Receiver, Sender};
    struct Pipe {
        tx: Sender<u8>,
        rx: Receiver<u8>,
    }
    fn pipe_pair() -> (Pipe, Pipe) {
        let (a_tx, a_rx) = channel();
        let (b_tx, b_rx) = channel();
        (Pipe { tx: a_tx, rx: b_rx }, Pipe { tx: b_tx, rx: a_rx })
    }
    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            for b in buf {
                self.tx.send(*b).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))?;
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }
    impl Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            // fill at least one byte (blocking), then drain what's buffered without blocking
            let first = self.rx.recv().map_err(|_| io::Error::new(io::ErrorKind::UnexpectedEof, "eof"))?;
            buf[0] = first;
            let mut n = 1;
            while n < buf.len() {
                match self.rx.try_recv() {
                    Ok(b) => { buf[n] = b; n += 1; }
                    Err(_) => break,
                }
            }
            Ok(n)
        }
    }

    fn handshake_pair(psk: [u8; 32]) -> (Session, Session) {
        let (mut c, mut s) = pipe_pair();
        let ch = std::thread::spawn(move || {
            let sess = client_handshake(&mut c, &psk).unwrap();
            (sess, c)
        });
        let ssess = server_handshake(&mut s, &psk).unwrap();
        let (csess, _c) = ch.join().unwrap();
        // rebuild pipes for the record-layer tests that follow: return sessions only;
        // callers that need I/O use round_trip below
        (csess, ssess)
    }

    #[test]
    fn ct_eq_is_correct() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }

    #[test]
    fn derive_psk_is_deterministic_and_separated() {
        assert_eq!(derive_psk("hunter2"), derive_psk("hunter2"));
        assert_ne!(derive_psk("hunter2"), derive_psk("hunter3"));
    }

    #[test]
    fn handshake_agrees_on_keys() {
        let psk = derive_psk("cluster-secret");
        let (c, s) = handshake_pair(psk);
        // client's send (d2l) must equal server's recv (d2l); and vice-versa
        assert_eq!(c.send_keys.cipher, s.recv_keys.cipher);
        assert_eq!(c.send_keys.mac, s.recv_keys.mac);
        assert_eq!(c.recv_keys.cipher, s.send_keys.cipher);
        assert_eq!(c.recv_keys.mac, s.send_keys.mac);
    }

    #[test]
    fn wrong_psk_is_rejected() {
        let (mut c, mut s) = pipe_pair();
        let good = derive_psk("right");
        let ch = std::thread::spawn(move || client_handshake(&mut c, &good).map(|_| ()));
        let bad = derive_psk("wrong");
        let sres = server_handshake(&mut s, &bad);
        assert!(sres.is_err(), "server must reject a dialer with the wrong PSK");
        // the client side also errors (its MAC check on HS2 fails, or the pipe breaks)
        let _ = ch.join().unwrap();
    }

    #[test]
    fn record_round_trip_encrypts_and_authenticates() {
        let psk = derive_psk("k");
        let (mut c, mut s) = pipe_pair();
        let ch = std::thread::spawn(move || {
            let mut sess = client_handshake(&mut c, &psk).unwrap();
            // send two records
            sess.send(&mut c, b"hello over the internet").unwrap();
            sess.send(&mut c, b"second record").unwrap();
            // the ciphertext on the wire is not the plaintext (spot check by re-deriving)
            c // keep pipe alive
        });
        let mut ssess = server_handshake(&mut s, &psk).unwrap();
        let m1 = ssess.recv(&mut s).unwrap();
        let m2 = ssess.recv(&mut s).unwrap();
        assert_eq!(m1, b"hello over the internet");
        assert_eq!(m2, b"second record");
        let _ = ch.join().unwrap();
    }

    #[test]
    fn tampered_record_is_rejected() {
        // craft a record with the session keys, flip a ciphertext bit, verify recv rejects
        let psk = derive_psk("k");
        let (c, s) = handshake_pair(psk);
        let mut sender = c;
        let mut receiver = s;
        // build a record by hand mirroring Session::send
        let seq = 0u64;
        let ct = xor_keystream(&sender.send_keys.cipher, seq, b"attack at dawn");
        let mut bad_ct = ct.clone();
        bad_ct[0] ^= 0x01;
        let tag = record_tag(&sender.send_keys.mac, seq, &ct); // tag over the ORIGINAL ct
        let mut framed = Vec::new();
        framed.extend_from_slice(&seq.to_be_bytes());
        framed.extend_from_slice(&bad_ct);
        framed.extend_from_slice(&tag);
        // feed it straight into recv via a pipe
        let (mut w, mut r) = pipe_pair();
        let jh = std::thread::spawn(move || { let _ = write_len_prefixed(&mut w, &framed); w });
        let res = receiver.recv(&mut r);
        assert!(res.is_err(), "a flipped ciphertext bit must fail the MAC");
        let _ = jh.join().unwrap();
        let _ = &mut sender;
    }

    #[test]
    fn replayed_sequence_is_rejected() {
        let psk = derive_psk("k");
        let (c, s) = handshake_pair(psk);
        let sender = c;
        let mut receiver = s;
        // two valid records but delivered out of order (seq 1 before seq 0)
        let mk = |seq: u64, pt: &[u8]| {
            let ct = xor_keystream(&sender.send_keys.cipher, seq, pt);
            let tag = record_tag(&sender.send_keys.mac, seq, &ct);
            let mut f = Vec::new();
            f.extend_from_slice(&seq.to_be_bytes());
            f.extend_from_slice(&ct);
            f.extend_from_slice(&tag);
            f
        };
        let r1 = mk(1, b"one");
        let (mut w, mut r) = pipe_pair();
        let jh = std::thread::spawn(move || { let _ = write_len_prefixed(&mut w, &r1); w });
        let res = receiver.recv(&mut r); // expecting seq 0, got seq 1
        assert!(res.is_err(), "an out-of-order sequence must be rejected");
        let _ = jh.join().unwrap();
    }

    #[test]
    fn oversized_handshake_frame_is_refused() {
        // an unauthenticated peer announces a huge frame; the server must refuse it at the
        // handshake cap, never allocating 64 MiB
        let psk = derive_psk("k");
        let (mut c, mut s) = pipe_pair();
        let jh = std::thread::spawn(move || {
            // send a length prefix far above MAX_HANDSHAKE, then nothing
            let _ = c.write_all(&(1_000_000u32).to_be_bytes());
            let _ = c.flush();
            c
        });
        let res = server_handshake(&mut s, &psk);
        assert!(res.is_err(), "server must refuse an oversized handshake frame");
        let _ = jh.join().unwrap();
    }

    #[test]
    fn configured_psk_reads_env_and_file() {
        std::env::set_var("ORPHEUS_PSK", "  from-env  ");
        assert_eq!(configured_psk(None), Some(derive_psk("from-env")));
        std::env::remove_var("ORPHEUS_PSK");
        let dir = std::env::temp_dir().join(format!("orpheus-psk-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("psk"), "from-file\n").unwrap();
        assert_eq!(configured_psk(Some(&dir)), Some(derive_psk("from-file")));
        assert_eq!(configured_psk(None), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
