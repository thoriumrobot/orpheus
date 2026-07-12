#!/bin/sh
# ============================================================================
# build-apk.sh — build the Termux-free Orpheus app for Android.
#
# WHAT THIS PRODUCES
#   android/app/build/outputs/apk/release/app-release-unsigned.apk
#   — a normal app you tap to launch. It contains ONE thing that matters: the
#   static `latte` binary, shipped as lib/arm64-v8a/liblatte.so.
#
# WHY liblatte.so
#   Android has forbidden executing files from an app's writable data dir since
#   API 29 (W^X). The installer, however, extracts everything under lib/<abi>/
#   in the APK into nativeLibraryDir WITH THE EXECUTE BIT. That directory is
#   read-only to the app. So a binary named `liblatte.so` is extracted, marked
#   executable, and run — no root, no Termux, no writable-exec anywhere. The
#   name is a packaging convention, not a claim about the file's format.
#
# WHY THIS WORKS AT ALL
#   Orpheus is one static, zero-dependency executable. The .lat libraries were
#   always embedded; src/site.rs now embeds the GUI's pages too. So there is
#   nothing to install beside the binary — the APK is the whole system.
#
# NO rustc ON THE PHONE
#   Correct, and fine: Anvil stands down and the interpreter + JIT are the
#   engine (`latte android` says so on startup). src/loom.rs raises the step
#   budget when no rustc is present, so the finance/ML models finish rather
#   than dying with OutOfFuel. Cached native binaries, if any were pulled from
#   a peer, still run.
#
# USAGE
#   ./build-apk.sh            # build binary + APK (needs the Android SDK)
#   ./build-apk.sh --binary   # just stage the binary into android/app/src/main/jniLibs
#
# REQUIREMENTS
#   * the aarch64 static binary — this script builds it via ./build-android.sh
#   * for the APK step: ANDROID_HOME (or ANDROID_SDK_ROOT) with build-tools and
#     platform 34, plus a JDK. Gradle fetches the plugin on first run.
#
# INSTALL
#   apksigner sign --ks <your.keystore> --out orpheus.apk app-release-unsigned.apk
#   adb install orpheus.apk
#   (or build a debug APK: `./gradlew assembleDebug`, which self-signs.)
# ============================================================================
set -e
cd "$(dirname "$0")"

BIN=target-static/aarch64-unknown-linux-gnu/release/latte

# ---- 1. the engine ---------------------------------------------------------
if [ ! -f "$BIN" ]; then
    echo "building the static aarch64 binary first…"
    ./build-android.sh --static
fi

# ---- 2. stage it as a "native library" -------------------------------------
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp "$BIN" android/app/src/main/jniLibs/arm64-v8a/liblatte.so
echo "staged: android/app/src/main/jniLibs/arm64-v8a/liblatte.so ($(wc -c < "$BIN") bytes)"

if [ "$1" = "--binary" ]; then
    echo "binary staged; run gradle yourself, or re-run without --binary."
    exit 0
fi

# ---- 3. the APK ------------------------------------------------------------
SDK="${ANDROID_HOME:-$ANDROID_SDK_ROOT}"
if [ -z "$SDK" ] || [ ! -d "$SDK" ]; then
    cat >&2 <<'EOM'

No Android SDK found (set ANDROID_HOME or ANDROID_SDK_ROOT).

The binary is staged, so you can finish the APK anywhere the SDK lives:
    cd android && ./gradlew assembleDebug        # self-signed, installable now
    cd android && ./gradlew assembleRelease      # then sign with apksigner

Or skip the app entirely and sideload the binary — the GUI is embedded, so this
is enough for a full Orpheus on the phone:
    adb push target-static/aarch64-unknown-linux-gnu/release/latte /data/local/tmp/latte
    adb shell 'chmod +x /data/local/tmp/latte && HOME=/data/local/tmp /data/local/tmp/latte android'
    adb forward tcp:8088 tcp:8088    # then open http://127.0.0.1:8088/ on the PC
EOM
    exit 1
fi

cd android
if [ -x ./gradlew ]; then
    ./gradlew assembleRelease
else
    gradle assembleRelease
fi
echo
echo "done: android/app/build/outputs/apk/release/app-release-unsigned.apk"
echo "sign it:  apksigner sign --ks <your.keystore> --out orpheus.apk app-release-unsigned.apk"
echo "install:  adb install orpheus.apk"
