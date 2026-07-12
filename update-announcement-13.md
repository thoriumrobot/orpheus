# Orpheus update — the Android app, honest interpretation, and a secure channel

## An Android app, with no Termux

`./build-apk.sh` now produces a real app you tap to launch. `android/` holds the whole
thing: one Activity, one WebView, and no Android-specific logic beyond starting a process
and showing a view.

The trick is not a trick. Android has forbidden executing files from an app's writable
data directory since API 29, but the installer still extracts everything an APK ships
under `lib/<abi>/` into `nativeLibraryDir` **with the execute bit**, in a directory the
app cannot write. So Orpheus ships as `lib/arm64-v8a/liblatte.so` — the ELF executable
wearing the name the packager requires. Nothing is ever written to a writable-executable
location: policy-compliant, no root, no toolchain on the device.

This became possible because the binary is now genuinely everything. The `.lat` libraries
were always compiled in; **`src/site.rs` embeds the GUI's 26 pages and fonts too**. A
single 4.8 MB file serves the whole console with no repository beside it — verified by
running the server from an empty directory. (`--root` still wins when a real `lib/site`
exists, so editing a page and reloading works exactly as before.) Sideloading the bare
binary is therefore also a complete Orpheus: `latte android` binds loopback, prints its
URL, and serves.

## Interpretation, corrected

"No rustc on a phone, so the interpreter is the engine" was true in principle and false in
practice. The evaluator's step budget (`DEFAULT_FUEL`, 50M) exists to stop a runaway
`*f f`, and it was sized for a machine where anything heavy gets handed to Anvil's native
compiler. On an interpret-only device the interpreter *is* the heavy path — so the finance
and ML tools died with `OutOfFuel` instead of running. That is not "interpretation works".

`src/loom.rs` now decides the budget by asking whether a `rustc` exists: `DEFAULT_FUEL`
when the native path can absorb the work, `INTERPRETER_FUEL` (20G) when it cannot, and
`ORPHEUS_FUEL=<n>` (`0` = unlimited) for explicit control. The guard stays finite, so
non-termination still stops rather than hanging the GUI.

Verified on real aarch64 under emulation with `rustc` hidden and no `lib/` directory: the
bond model's `(breport 0)` returns exactly what the native path returns —
`[[0 986] [[0 696] [[0 633] [1 0]]]]`, 98.6% train / 69.6% out-of-sample / 63.3% baseline
— in about a minute rather than two seconds. Slower, identical, correct. The whole GUI
answers over HTTP from the same emulated binary.

## Security for instances over the Internet

Both network surfaces assumed a trusted LAN. The gossip channel was framed plaintext with
no authentication: anyone who could reach the port could read a node's entire event log
and inject forged events. The web console exposes `eval`, the ledger, and the editor to
anyone who can reach it.

Both are now protected from one shared secret, using only the Keccak primitives already in
the trusted base — `src/sha3.rs` gained SHAKE256 (NIST vector verified) and a keyed digest;
`src/secure.rs` is the only new code to audit. No OpenSSL, nothing vendored.

**The channel.** A pre-shared key (`ORPHEUS_PSK`, a `psk` file, or `--psk`) drives a mutual
challenge–response handshake over fresh nonces — the PSK is never sent, both MACs are
checked in constant time, and both nonces feed every derived key, so recorded sessions
cannot be replayed. Records are encrypt-then-MAC with a per-direction sequence number:
tamper, reorder, truncation, and replay are all rejected, and the keystream is never
reused. A peer that cannot prove the PSK never reaches the sync loop, so it can neither
read the log nor write to it. A plaintext dialer cannot downgrade a secured node. With no
PSK the legacy plaintext protocol still serves a trusted LAN — and a non-loopback listener
without one now warns loudly.

**The console.** Reachable off-box, it requires a token: `ORPHEUS_TOKEN`, or one derived
from the PSK so a single secret unlocks both surfaces. With neither, a public bind is
**refused** rather than silently exposed. The token arrives as a bearer header, an
`X-Orpheus-Token` header, or once as `?token=` (which plants an `HttpOnly; SameSite=Strict`
cookie); comparison is constant time and the gate runs before routing, so no handler can
leak state to an unauthenticated caller.

`docs/security.md` states the model plainly, including what it does *not* give: PSK holders
are full peers, there is no forward secrecy across a PSK compromise, the GUI token is a
bearer credential over whatever transport you provide, and record lengths are unpadded.
Scope decisions, not oversights.

## Tests

SHAKE256 against its NIST vector, the streaming/prefix property, key–message separation;
constant-time compare; handshake key agreement, wrong-PSK rejection, encrypt-then-MAC round
trip, tamper detection, sequence-violation rejection, PSK discovery from env and file. Over
real TCP: two nodes converging across an encrypted channel, an intruder with the wrong PSK
failing to inject events *or* learn state, and a plaintext dialer refused by a secured node.
Loopback-address classification. The token gate across every channel a phone can use, and
its rejections. The embedded site resolving with no root present, a real root still winning,
traversal still refused, and the embedded bytes matching the repository's files. The fuel
decision table. Plus the full suite green, and `xarch_test.sh` unchanged.
