# Orpheus on Android

Orpheus is a single static-linkable binary with zero dependencies beyond the
Rust standard library, and Android is a Linux kernel — so the whole system
runs on a phone: the Latte language, the interpreter + JIT, the GUI (served
over HTTP to the phone's own browser), the gossip layer, and the distributed
execution layer. A phone is a full peer: it can host persistent nodes, serve
as a worker, and coordinate work running on PCs — and vice versa.

## The app: tap an icon, no Termux

```sh
./build-apk.sh                # builds the aarch64 binary, stages it, assembles the APK
adb install orpheus.apk       # tap to launch; the GUI opens inside the app
```

`android/` is the whole app: one Activity, one WebView, no Android-specific logic beyond
starting a process and showing a view.

**How it runs a binary at all.** Android has forbidden executing files from an app's
writable data directory since API 29 (W^X). The one place the platform still puts an
executable file is `nativeLibraryDir`: the installer extracts everything the APK ships
under `lib/<abi>/` there, *with the execute bit*, into a directory the app cannot write.
So Orpheus ships as `lib/arm64-v8a/liblatte.so` — not a shared object, but the ELF
executable wearing the name the packager requires. The installer extracts and marks it
executable; `MainActivity` runs it with `ProcessBuilder`. Nothing is ever written to a
writable-executable location: this is policy-compliant, not a workaround, and it needs no
root, no Termux, and no toolchain on the device.

`latte android` then prints one line — `ORPHEUS_URL http://127.0.0.1:8088/` — and the
Activity points its WebView at it.

**Storage, ports, permissions.**

- The app passes `--home <filesDir>/orpheus`. `HOME` already governs every cache Orpheus
  keeps (market series, news wire, Anvil program cache, node store), so all of it lands
  inside the sandbox and no storage permission is needed.
- `latte android` binds **loopback only** and picks a free port if 8088 is taken. A phone
  is usually on someone else's Wi-Fi and the GUI exposes `eval` and the ledger, so nothing
  on the network can reach it. Peers reach the node through `latte net` with a pre-shared
  key (`docs/security.md`), never through the web console.
- The manifest declares **no permissions**. Add `INTERNET` only if you want the news wire to
  fetch feeds or the node to gossip with remote peers — and read `docs/security.md` first.
  `network_security_config.xml` permits cleartext to `127.0.0.1` only.

**Why an APK is possible now.** The `.lat` libraries were always compiled into the binary;
`src/site.rs` embeds the GUI's pages and `src/docs_embed.rs` embeds the documentation, so
the **Docs** index in the GUI is fully populated on the phone too (with no `docs/` directory
beside the executable it falls back to the embedded copies). The executable is the entire
system, so there is nothing to install beside it. (`serve` still prefers a real `--root` /
`docs/` directory when one exists, so editing a page and reloading works exactly as before.)

## Getting it onto a phone, the other ways

**Termux (still supported).** Install [Termux](https://f-droid.org/packages/com.termux/)
from F-Droid, then build on the device — Termux's rustc targets Android
natively and Orpheus has no other dependencies:

```sh
pkg install rust binutils
# copy the source over (git clone, or termux-setup-storage + your Downloads)
cd orpheus && ./build-android.sh        # detects Termux, plain cargo build
```

**Sideload a static binary (no toolchain on the phone).** On a PC with
`gcc-aarch64-linux-gnu` and rust sources installed (Debian/Ubuntu:
`apt install gcc-aarch64-linux-gnu rust-src`; rustup:
`rustup component add rust-src`):

```sh
./build-android.sh --static             # -Z build-std cross build
adb push target-static/aarch64-unknown-linux-gnu/release/latte /data/local/tmp/latte
adb shell 'chmod +x /data/local/tmp/latte; HOME=/data/local/tmp /data/local/tmp/latte eval "(mul 6 7)"'
```

The static binary is fully self-contained (~4 MB) and also runs inside
Termux. On a bare `adb shell`, set `HOME` (and `TMPDIR`) to a writable
directory such as `/data/local/tmp` — Orpheus falls back to the temp dir for
its caches when `$HOME` is not writable.

The binary is everything — the GUI's pages are embedded (`src/site.rs`), so nothing needs
to be pushed alongside it:

```sh
adb shell 'cd /data/local/tmp && HOME=$PWD ./latte android'
adb forward tcp:8088 tcp:8088     # then open http://127.0.0.1:8088/ on the PC
```

(A `lib/site` directory beside the binary still wins when present, which is what makes
editing a page and reloading work during development.)

**PC with the Android NDK.** `export ANDROID_NDK_HOME=…`,
`rustup target add aarch64-linux-android`, then `./build-android.sh` builds a
bionic-linked binary.

## The GUI on a phone

```sh
latte gui                               # then open http://127.0.0.1:8088 in Chrome
```

In Termux, `latte gui` hands the URL to the system browser automatically
(`termux-open-url`, falling back to `am start`). Chrome's *Add to Home
screen* turns it into an app icon.

The System console keeps the Oberon model, adapted to fingers:

- **long-press a command line** to run it — the middle-click of phones
  (works in text frames, tool texts, and the log);
- **▶ Run** in the header runs the selection / caret line of the marked
  viewer — the button form of Ctrl/⌘+Enter;
- **drag** title bars and separators with a finger (all drags are pointer
  gestures now); **hold a title bar ~0.6 s without dragging** to lift the
  viewer into *move* mode (the middle inter-click of phones);
- below 720 px the two tracks stack vertically and the header link bar
  scrolls sideways; grab bars fatten on touch screens.

Every other page (editor, chess, charts, tools, learn, board) is tap-driven
already and carries a mobile viewport.

## Phones and PCs are equal peers

The GUI's **/network page** (docs/network-gui.md) works identically on a
phone: the ledger it hosts converges with any PC's, the connect/put/time-travel
forms are touch widgets, and the whole page refreshes itself as gossiped
events arrive — verified cross-architecture in this repository's tests. The
**/notes editor** (docs/collaborative-notes.md) rides along: shared notes,
plans, ballots, code, conlangs, and drawings, edited from the phone and
converging with every connected instance — the cross-architecture ledger
test's sibling was run against the notes node too.

The wire protocols — gossip (`latte node`) and evaluation tasks
(`latte worker`) — are architecture-independent (length-framed jammed nouns;
Latte programs are pure functions, so a task computes the same noun on any
CPU). Any instance can connect to any instance:

```sh
# phone as a WORKER for a PC (both on one Wi-Fi):
phone$ latte worker --listen 0.0.0.0:9700
pc$    latte workers add <phone-ip>:9700
pc$    latte ml linear --store /tmp/model      # rounds now train on the phone too

# PC as a worker for the PHONE (the coordinator can be the phone):
pc$    latte worker --listen 0.0.0.0:9700
phone$ ORPHEUS_WORKERS=<pc-ip>:9700 latte eval '(dmap (fn [x] -> (mul x x)) [ 1 [ 2 [ 3 0 ] ] ])'

# a persistent node on each, converging over gossip (set ORPHEUS_PSK first if
# these cross the Internet rather than one Wi-Fi — see docs/security.md):
pc$    latte node --listen 0.0.0.0:9000 --agent kv --store ~/pcstore
phone$ latte node --listen 0.0.0.0:9000 --peer <pc-ip>:9000 --agent kv --store ~/phonestore
```

This matrix is exercised by `xarch_test.sh` in the repository root: an x86-64
PC instance and an aarch64 instance (the Android build, run under
`qemu-aarch64` with no rustc and an isolated HOME — a faithful stand-in for a
phone) drive each other in all four directions. The gossip test asserts the
two stores converge to the **same content id** — byte-identical state across
architectures — and the FedAvg test trains one model with shards running on
both CPUs at once, zero fallbacks.

## What changes on a device without rustc

Nothing that affects answers. The adaptive engine's native path (Anvil
compiles hot programs with `rustc`) requires a toolchain; on a phone without
one, Orpheus detects this once and the native engine stands down cleanly:
already-cached binaries still run, new builds are skipped silently, and the
interpreter + JIT (pure Rust closures — any CPU) answer everything.
`latte profile` reports it honestly:

```
native      — (no rustc on this device; the interpreter + JIT is the engine)
```

One thing *did* have to change for that to be true in practice. The evaluator's step budget
guards against a runaway `*f f`, and it was sized for a machine where anything heavy gets
handed to the native compiler. On an interpret-only device the interpreter **is** the heavy
path, so that ceiling silently turned the finance and ML tools into `OutOfFuel` errors.
`src/loom.rs` now decides the budget by asking whether a `rustc` exists:

| device | budget | why |
|---|---|---|
| has `rustc` | `DEFAULT_FUEL` (50M) | the native path absorbs the heavy programs |
| no `rustc` (a phone) | `INTERPRETER_FUEL` (20G) | the interpreter must finish the work itself |
| `ORPHEUS_FUEL=<n>` | `n` (`0` = unlimited) | explicit control for scripts and batch jobs |

The guard stays finite, so a non-terminating program still stops rather than hanging the
GUI. Verified on real aarch64 under emulation with `rustc` hidden and no `lib/` directory:
the bond model's `(breport 0)` returns exactly what the native path returns —
`[[0 986] [[0 696] [[0 633] [1 0]]]]` (98.6% train / 69.6% out-of-sample / 63.3% baseline)
— in about a minute rather than two seconds. Slower, identical, correct.

In Termux with `pkg install rust`, the native engine works exactly as on a
PC, building aarch64 binaries into the on-device cache.

## Practical notes

- **Keep it awake:** Termux pauses on deep sleep — run `termux-wake-lock`
  (Termux:API) while hosting a worker or node, or keep Termux foregrounded.
- **Find the phone's IP:** `ifconfig` in Termux (`pkg install net-tools`), or
  Settings → Wi-Fi. Workers/nodes should listen on `0.0.0.0` to accept LAN
  peers.
- **Battery:** distributed training is CPU work; the coordinator's per-chunk
  local fallback means a phone dropping off Wi-Fi mid-round costs nothing but
  time.
- **Ports:** Android permits unprivileged ports (>1024) without root — the
  defaults (8088 GUI, 9000 node, 9700 worker) all qualify.
