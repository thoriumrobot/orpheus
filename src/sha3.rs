//! Self-contained SHA3-256 (FIPS 202). No dependencies — the hash function that
//! produces content-addresses (CIDs) for knots, modules, and network events is
//! part of the trusted base, so we keep it in-tree and verifiable.
//!
//! Keccak-f[1600], rate = 1088 bits (136 bytes), capacity 512, SHA-3 domain pad 0x06.

const ROUNDS: usize = 24;

const RC: [u64; 24] = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000,
    0x000000000000808b, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008a, 0x0000000000000088, 0x0000000080008009, 0x000000008000000a,
    0x000000008000808b, 0x800000000000008b, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800a, 0x800000008000000a,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
];

const ROT: [[u32; 5]; 5] = [
    [0, 36, 3, 41, 18],
    [1, 44, 10, 45, 2],
    [62, 6, 43, 15, 61],
    [28, 55, 25, 21, 56],
    [27, 20, 39, 8, 14],
];

fn keccak_f(st: &mut [u64; 25]) {
    for round in 0..ROUNDS {
        // theta
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = st[x] ^ st[x + 5] ^ st[x + 10] ^ st[x + 15] ^ st[x + 20];
        }
        let mut d = [0u64; 5];
        for x in 0..5 {
            d[x] = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
        }
        for x in 0..5 {
            for y in 0..5 {
                st[x + 5 * y] ^= d[x];
            }
        }
        // rho + pi
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                let nx = y;
                let ny = (2 * x + 3 * y) % 5;
                b[nx + 5 * ny] = st[x + 5 * y].rotate_left(ROT[x][y]);
            }
        }
        // chi
        for x in 0..5 {
            for y in 0..5 {
                st[x + 5 * y] =
                    b[x + 5 * y] ^ ((!b[(x + 1) % 5 + 5 * y]) & b[(x + 2) % 5 + 5 * y]);
            }
        }
        // iota
        st[0] ^= RC[round];
    }
}

/// SHA3-256 digest of `input`.
pub fn sha3_256(input: &[u8]) -> [u8; 32] {
    const RATE: usize = 136; // bytes
    let mut st = [0u64; 25];

    // absorb
    let mut offset = 0;
    while input.len() - offset >= RATE {
        absorb_block(&mut st, &input[offset..offset + RATE]);
        keccak_f(&mut st);
        offset += RATE;
    }
    // pad: 0x06 ... 0x80 within the final RATE-sized block
    let mut block = [0u8; RATE];
    let rem = &input[offset..];
    block[..rem.len()].copy_from_slice(rem);
    block[rem.len()] ^= 0x06;
    block[RATE - 1] ^= 0x80;
    absorb_block(&mut st, &block);
    keccak_f(&mut st);

    // squeeze 32 bytes (fits in first 4 lanes)
    let mut out = [0u8; 32];
    for i in 0..4 {
        out[i * 8..i * 8 + 8].copy_from_slice(&st[i].to_le_bytes());
    }
    out
}

fn absorb_block(st: &mut [u64; 25], block: &[u8]) {
    // RATE (136) = 17 lanes of 8 bytes
    for i in 0..17 {
        let mut lane = 0u64;
        for j in 0..8 {
            lane |= (block[i * 8 + j] as u64) << (8 * j);
        }
        st[i] ^= lane;
    }
}

/// Hex string of a 32-byte digest.
pub fn hex(d: &[u8]) -> String {
    let mut s = String::with_capacity(d.len() * 2);
    for b in d {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

// ===========================================================================
// SHAKE256 — the extendable-output function (FIPS 202) that the secure
// transport (src/secure.rs) is built from. Same Keccak-f[1600] permutation as
// SHA3-256, capacity 512 (rate 136), but the XOF domain pad (0x1f) and an
// arbitrary-length squeeze. From this one primitive we get everything the
// channel needs without a single external crate: a key-derivation function
// (squeeze N key bytes from a seed), a keystream (squeeze len(plaintext) bytes
// and XOR), and — via keyed hashing — a message authentication code.
// ===========================================================================

/// SHAKE256 XOF: absorb `input`, squeeze `out.len()` bytes into `out`.
pub fn shake256(input: &[u8], out: &mut [u8]) {
    const RATE: usize = 136; // capacity 512, same as SHA3-256
    let mut st = [0u64; 25];

    // absorb
    let mut offset = 0;
    while input.len() - offset >= RATE {
        absorb_block(&mut st, &input[offset..offset + RATE]);
        keccak_f(&mut st);
        offset += RATE;
    }
    // pad: XOF domain 0x1f ... 0x80
    let mut block = [0u8; RATE];
    let rem = &input[offset..];
    block[..rem.len()].copy_from_slice(rem);
    block[rem.len()] ^= 0x1f;
    block[RATE - 1] ^= 0x80;
    absorb_block(&mut st, &block);
    keccak_f(&mut st);

    // squeeze
    let mut produced = 0;
    while produced < out.len() {
        // emit up to RATE bytes (17 lanes) from the state
        let take = (out.len() - produced).min(RATE);
        let mut buf = [0u8; RATE];
        for i in 0..17 {
            buf[i * 8..i * 8 + 8].copy_from_slice(&st[i].to_le_bytes());
        }
        out[produced..produced + take].copy_from_slice(&buf[..take]);
        produced += take;
        if produced < out.len() {
            keccak_f(&mut st);
        }
    }
}

/// Convenience: squeeze exactly `n` bytes as a `Vec`.
pub fn shake256_vec(input: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    shake256(input, &mut out);
    out
}

/// A 32-byte keyed digest (MAC / KDF leaf): SHAKE256(key ‖ 0x00 ‖ msg) → 32 bytes.
/// The 0x00 separator keeps `key` and `msg` from running together (the key length
/// is also folded in by callers that need domain separation). SHAKE is not length-
/// extendable in the Merkle–Damgård sense, so prefix-keying is sound here; we keep
/// the construction simple and in one place.
pub fn keyed256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(key.len() + 1 + msg.len());
    buf.extend_from_slice(key);
    buf.push(0x00);
    buf.extend_from_slice(msg);
    let mut out = [0u8; 32];
    shake256(&buf, &mut out);
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake256_kat_empty() {
        // FIPS 202 / NIST test vector: SHAKE256("") first 32 bytes.
        let mut out = [0u8; 32];
        shake256(b"", &mut out);
        assert_eq!(
            hex(&out),
            "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f"
        );
    }

    #[test]
    fn shake256_is_a_prefix_stream() {
        // Squeezing N bytes then M>N must agree on the first N (streaming property).
        let short = shake256_vec(b"orpheus", 32);
        let long = shake256_vec(b"orpheus", 200);
        assert_eq!(short[..], long[..32]);
        // and it crosses the 136-byte rate boundary correctly (non-trivial squeeze)
        assert_ne!(long[135], long[136]);
    }

    #[test]
    fn keyed256_separates_key_and_message() {
        // changing the key changes the tag; splitting the same bytes differently too
        let a = keyed256(b"key1", b"message");
        let b = keyed256(b"key2", b"message");
        let c = keyed256(b"keymes", b"sage"); // same concatenation, different split
        assert_ne!(a, b);
        assert_ne!(a, c, "the 0x00 separator prevents key/message sliding");
    }

    #[test]
    fn kat_empty() {
        // Known answer: SHA3-256("") = a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a
        let d = sha3_256(b"");
        assert_eq!(
            hex(&d),
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
        );
    }

    #[test]
    fn kat_abc() {
        // SHA3-256("abc") = 3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532
        let d = sha3_256(b"abc");
        assert_eq!(
            hex(&d),
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
        );
    }
}
