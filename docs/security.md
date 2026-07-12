# Security — connecting Orpheus instances over the Internet

Orpheus has two network surfaces, and until now both assumed a trusted LAN:

* the **gossip channel** (`latte net`, the ledger, notes, chess) — framed plaintext over
  TCP, no authentication. Anyone who could reach the port could read a node's entire event
  log and inject forged events into it.
* the **web GUI** (`latte serve`, `latte gui`) — which exposes `eval`, the ledger, the
  editor, and the trading tools to anyone who can reach the port.

Both are now protected, from a single shared secret, using only the Keccak primitives that
were already in the trusted base (`src/sha3.rs`). No OpenSSL, no vendored crate, nothing
new to audit but `src/secure.rs`.

## The trust model: a pre-shared key

Instances that federate are run by people who already share something out of band — a
cluster passphrase, a deployment secret. That is the root of trust here:

```
export ORPHEUS_PSK='a long high-entropy deployment secret'
# or:  echo 'secret' > <node-dir>/psk        # discovered automatically
# or:  latte net --psk 'secret' --listen 0.0.0.0:9000 --peer other.host:9000
```

What this gives you, stated honestly:

* **Confidentiality and mutual authentication** against network attackers who do not hold
  the PSK — the realistic Internet threat. A peer that cannot complete the handshake never
  reaches the sync loop, so it can neither read the log nor write to it.
* It is **symmetric, not public-key**. Everyone holding the PSK is a full peer, and the PSK
  must be distributed securely out of band. This is not a public federation protocol, and it
  does not claim forward secrecy if the PSK itself leaks. (The record framing leaves room
  for an ephemeral-DH upgrade that would add it.)

## The handshake

Mutual challenge–response over fresh nonces, so nothing is replayable and the PSK is never
sent:

```
dialer   → listener :  version ‖ nonce_d
listener → dialer   :  nonce_l ‖ mac_l          mac_l = keyed256(PSK, "L" ‖ transcript)
dialer   → listener :  mac_d                    mac_d = keyed256(PSK, "D" ‖ transcript)

transcript = version ‖ nonce_d ‖ nonce_l
```

Both MACs are verified in **constant time**. Because both fresh nonces feed the transcript,
a recorded session cannot be replayed against a new one. Session keys are then derived per
direction — `shake256(PSK ‖ "orpheus-sec-v1|d2l" ‖ transcript, 64)` and its `l2d` twin —
and split into a cipher key and a MAC key.

## The record layer

Encrypt-then-MAC, with a per-direction sequence number:

```
record = seq (8B) ‖ ct ‖ tag (16B)
ct  = plaintext ⊕ SHAKE256(cipher_key ‖ seq, len(plaintext))
tag = keyed256(mac_key, seq ‖ ct)[..16]
```

The receiver verifies the tag **before** decrypting, and requires a strictly increasing
`seq`. So records cannot be tampered with, reordered, truncated, or replayed. The keystream
is never reused: `seq` is unique per direction, and the two directions hold different keys.

## Resource-exhaustion limits

Because the listener accepts connections from the open Internet, it bounds what an
*unauthenticated* peer can cost before it has proven the PSK:

* **Frame size.** Every length-prefixed frame is refused above 64 MiB. During the handshake
  the cap is far tighter — 128 bytes — so a peer cannot announce a huge length and force a
  large allocation before authenticating (a memory-amplification DoS otherwise).
* **Handshake time.** The handshake runs under a 15-second read/write timeout, so a peer
  that connects and then stalls or dribbles bytes cannot pin a connection thread
  indefinitely (a slow-loris). The timeout is lifted once the session is established,
  because the steady gossip loop is legitimately long-lived and idle between a peer's
  events.

## The web GUI

The console is gated by a token whenever it is reachable off-box:

* `ORPHEUS_TOKEN=<secret>` sets one explicitly.
* Otherwise a token is **derived from the PSK** — `shake256("orpheus-gui-token-v1" ‖ psk)`,
  16 bytes of hex — so one secret unlocks both surfaces and a phone that knows the cluster
  secret can compute its own access URL.
* With **neither**, a bind to anything but loopback is **refused**, not silently exposed.

The token may be presented as `Authorization: Bearer <tok>`, `X-Orpheus-Token: <tok>`, or
once as `?token=<tok>` (the server then sets an `HttpOnly; SameSite=Strict` cookie so
in-page fetches work). Comparison is constant time; the gate runs **before routing**, so no
handler can leak state to an unauthenticated caller. Unauthenticated requests get a `401`
with a `WWW-Authenticate` header — never a redirect, which would leak the token in a
`Location`.

## What is still plaintext, and when that is fine

With **no PSK configured**, the gossip channel keeps the legacy plaintext protocol, so a
trusted LAN or a loopback-only setup keeps working unchanged. A listener bound to a
non-loopback address without a PSK prints a loud warning, because that is exactly the
dangerous case. A plaintext dialer cannot sync with a PSK-protected node: the downgrade is
refused, not negotiated.

The news wire's outbound feed fetches go through `curl` and use whatever TLS the system
provides; nothing in Orpheus weakens that.

## Deployment recipes

**Two machines over the Internet, both directions authenticated and encrypted:**

```
# on each host
export ORPHEUS_PSK='…'
latte net --listen 0.0.0.0:9000 --peer other.example:9000 --store ./node -v
#  prints: secure transport ENABLED (pre-shared key; peers are mutually authenticated and encrypted)
```

**A phone reaching a home node's GUI.** Do *not* expose the console to the Internet. Either

* keep the GUI on loopback and reach it through an SSH tunnel or a WireGuard/Tailscale
  interface (the GUI then sees a loopback client), or
* if you must bind publicly, set `ORPHEUS_TOKEN` (or a PSK) and put a TLS terminator in
  front of it — the token protects access, not the transport, and HTTP is HTTP.

**The Android app** binds loopback only and is reachable solely by its own WebView, so it
needs no token. When that phone gossips with a remote node, that traffic is the `latte net`
channel, and it should carry a PSK.

## Threats this does not address

* A holder of the PSK is a full peer. There is no per-node authorization or revocation
  beyond rotating the PSK.
* No forward secrecy across a PSK compromise: recorded traffic can be decrypted by someone
  who later obtains the PSK.
* The GUI token is a bearer credential. Over plain HTTP on a hostile network it can be
  observed; use a tunnel or TLS terminator.
* Traffic analysis: record lengths are not padded, so message sizes are visible.

Each of these is a deliberate scope decision, not an oversight — the goal was a small,
auditable, dependency-free channel that makes the realistic Internet attacker (someone on
the path who does not hold your secret) unable to read your log or write to it.
