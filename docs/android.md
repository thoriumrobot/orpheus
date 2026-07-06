# Orpheus on Android

Orpheus is a single static-linkable binary with zero dependencies beyond the
Rust standard library, and Android is a Linux kernel — so the whole system
runs on a phone: the Latte language, the interpreter + JIT, the GUI (served
over HTTP to the phone's own browser), the gossip layer, and the distributed
execution layer. A phone is a full peer: it can host persistent nodes, serve
as a worker, and coordinate work running on PCs — and vice versa.

## Getting it onto a phone

**Termux (recommended).** Install [Termux](https://f-droid.org/packages/com.termux/)
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

The CLI, workers, and nodes need only the binary. The **GUI additionally
serves the `lib/site` directory from disk**, so push it alongside and run
from that directory (a Termux source build has it already):

```sh
adb push lib/site /data/local/tmp/lib/site
adb shell 'cd /data/local/tmp && HOME=$PWD ./latte gui'
```

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
events arrive — verified cross-architecture in this repository's tests.

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

# a persistent node on each, converging over gossip:
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
