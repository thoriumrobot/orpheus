# Orpheus update — the whole system runs on Android, and phones are full peers

## One source tree, one more pocket-sized computer

Orpheus has zero dependencies beyond the Rust standard library and no
platform-specific code, so the entire system now builds for and runs on
Android phones: Loom, the Latte language, the interpreter + JIT (pure Rust
closures — any CPU), the GUI, persistent gossiped nodes, and the distributed
execution layer. `build-android.sh` picks the right path by itself: a native
on-device build under Termux (`pkg install rust binutils`), a bionic-linked
build on a PC with the Android NDK, or — with nothing but Debian's
`gcc-aarch64-linux-gnu` and `rust-src` — a fully static ~4 MB aarch64 binary
via `-Z build-std` that sideloads with one `adb push` and runs on a bare
shell. The Debian rust-src package ships without the workspace lock that
build-std needs; the script restores the official one for the exact compiler
release automatically.

## The GUI learned to be touched

The System console keeps the Oberon model and adapts its gestures to
fingers. **Long-press a command line to run it** — the middle-click of
phones — in plain frames, tool texts, and the log (Android's long-press
`contextmenu` is intercepted only on coarse-pointer devices; right-click on a
desktop is untouched). A **▶ Run** button in the header executes the
selection or caret line of the marked viewer, the buttonless form of
Ctrl/⌘+Enter. Every drag — track separators, viewer separators, title-bar
resizes and moves — is now a pointer gesture, so fingers and mice share one
code path; **holding a title bar ~0.6 s without dragging lifts the viewer
into move mode** (the middle inter-click of phones). Below 720 px the two
tracks stack vertically, the header link bar scrolls sideways, and grab bars
fatten on touch screens. Track separators became axis-aware: the same drag
handler resizes columns on a desktop and rows on a stacked phone screen. The
`/board` page also gained the proper document head (doctype, viewport,
styles) it had been serving without.

## Phones without a toolchain lose nothing but the compiler

The adaptive engine's native path shells out to `rustc`; a sideloaded phone
binary has none. The engine now probes for the toolchain once and stands
down cleanly when it is absent: cached binaries still run, new builds are
skipped without per-call errors, background warms and daemon requests are
never attempted, and the interpreter + JIT answer everything — same results
on every machine. `latte profile` says it plainly: *no rustc on this device;
the interpreter + JIT is the engine.* Cache directories fall back to the
temp dir when `$HOME` is not writable (a bare `adb shell`), and
`latte gui` hands its URL to the phone's browser through `termux-open-url`
or the activity manager.

## Any instance connects to any instance

The wire protocols — gossip events and evaluation tasks — are length-framed
jammed nouns, independent of CPU architecture, and Latte programs are pure
functions, so a task computes the same noun everywhere. The new
`xarch_test.sh` proves the whole matrix with a real second architecture: an
x86-64 PC instance and the aarch64 Android build (run under `qemu-aarch64`
with no rustc and an isolated HOME — a faithful phone stand-in) drive each
other in all four directions. The PC distributes `dmap` chunks to the ARM
worker; the ARM instance coordinates work on the PC; two durable kv nodes
gossip across the architecture boundary and land on the **same content id**
— byte-identical state; and one FedAvg training run consolidates models
whose shards trained on both CPUs at once, zero fallbacks, one persistent
event.

Docs: `docs/android.md`. The x86-64 suite still passes whole, and the ARM
binary answers the same nouns the PC does — which is the only portability
statement a content-addressed system needs.
