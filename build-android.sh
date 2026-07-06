#!/bin/sh
# ============================================================================
# build-android.sh — build Orpheus for Android phones.
#
# Orpheus has zero dependencies beyond the Rust standard library, so it
# builds anywhere Rust does. Three ways, auto-selected:
#
#   1. ON the phone (Termux)     — plain `cargo build`: Termux's rustc
#      targets Android natively. The supported everyday path.
#   2. PC + the Android NDK      — a bionic-linked aarch64-linux-android
#      binary (needs `rustup target add aarch64-linux-android`).
#   3. PC + aarch64-linux-gnu-gcc + rust-src — a STATIC aarch64 binary via
#      `-Z build-std` (RUSTC_BOOTSTRAP). Fully self-contained: sideload it
#      with adb to /data/local/tmp, or run it in Termux, or test it on the
#      PC under `qemu-aarch64`. This is the path the repository's
#      cross-architecture interop tests use.
#
# Usage:  ./build-android.sh [--static|--dynamic]     (mode 3 default: static)
# ============================================================================
set -e
cd "$(dirname "$0")"

MODE=static
[ "$1" = "--dynamic" ] && MODE=dynamic

# ---- 1. on the phone itself (Termux) ---------------------------------------
if [ -n "$TERMUX_VERSION" ]; then
    echo "Termux detected — building natively on this phone (grab a coffee; ~10-20 min once)."
    echo "If rust is missing:  pkg install rust binutils"
    cargo build --offline --release 2>/dev/null || cargo build --release
    echo
    echo "done: target/release/latte"
    echo "try:  ./target/release/latte gui        then open http://127.0.0.1:8088 in Chrome"
    exit 0
fi

# ---- 2. PC with the Android NDK ---------------------------------------------
NDK="${ANDROID_NDK_HOME:-$ANDROID_NDK_ROOT}"
if [ -n "$NDK" ] && [ -d "$NDK" ]; then
    CLANG=$(ls "$NDK"/toolchains/llvm/prebuilt/*/bin/aarch64-linux-android21-clang 2>/dev/null | head -1)
    if [ -n "$CLANG" ]; then
        echo "Android NDK detected — building a bionic-linked aarch64-linux-android binary."
        rustup target add aarch64-linux-android 2>/dev/null || true
        CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG" \
            cargo build --release --target aarch64-linux-android
        echo "done: target/aarch64-linux-android/release/latte"
        echo "install:  adb push target/aarch64-linux-android/release/latte /data/local/tmp/ && adb shell chmod +x /data/local/tmp/latte"
        exit 0
    fi
fi

# ---- 3. PC with the GNU aarch64 toolchain + rust-src (build-std) -------------
if ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
    echo "No Termux, no NDK, no aarch64-linux-gnu-gcc." >&2
    echo "Install one of:" >&2
    echo "  · Termux on the phone:  pkg install rust binutils; then run this script there" >&2
    echo "  · the Android NDK:      export ANDROID_NDK_HOME=...; rustup target add aarch64-linux-android" >&2
    echo "  · Debian/Ubuntu cross:  apt install gcc-aarch64-linux-gnu rust-src (qemu-user to test)" >&2
    exit 1
fi
SRC="$(rustc --print sysroot)/lib/rustlib/src/rust"
if [ ! -d "$SRC" ]; then
    echo "rust std sources missing — install them:  rustup component add rust-src   (or apt install rust-src)" >&2
    exit 1
fi
if [ ! -f "$SRC/Cargo.lock" ]; then
    # Debian's rust-src package omits the workspace lock that -Z build-std
    # needs; the official one for this exact release restores it.
    REL=$(rustc --version | awk '{print $2}')
    echo "fetching the official Cargo.lock for rust $REL into the std source tree…"
    curl -sfL "https://raw.githubusercontent.com/rust-lang/rust/$REL/Cargo.lock" -o "$SRC/Cargo.lock" \
        || { echo "could not fetch Cargo.lock (offline?) — place rust $REL's Cargo.lock at $SRC/Cargo.lock" >&2; exit 1; }
fi
echo "cross-building a $MODE aarch64 binary with -Z build-std (RUSTC_BOOTSTRAP)…"
FLAGS=""
OUTDIR=target
if [ "$MODE" = "static" ]; then
    FLAGS="-C target-feature=+crt-static"
    OUTDIR=target-static
fi
RUSTC_BOOTSTRAP=1 RUSTFLAGS="$FLAGS" \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    cargo build --release --target aarch64-unknown-linux-gnu -Z build-std --target-dir "$OUTDIR"
BIN="$OUTDIR/aarch64-unknown-linux-gnu/release/latte"
echo
echo "done: $BIN"
if [ "$MODE" = "static" ]; then
    echo "sideload:  adb push $BIN /data/local/tmp/latte && adb shell 'chmod +x /data/local/tmp/latte; HOME=/data/local/tmp /data/local/tmp/latte eval \"(mul 6 7)\"'"
    echo "termux:    copy it anywhere in Termux and run it directly"
    echo "gui:       also push lib/site next to it (the GUI serves those pages from disk):"
    echo "           adb push lib/site /data/local/tmp/lib/site"
else
    echo "test here: qemu-aarch64 -L /usr/aarch64-linux-gnu $BIN eval '(mul 6 7)'"
fi
