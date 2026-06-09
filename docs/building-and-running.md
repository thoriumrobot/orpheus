# Building and running Orpheus on Ubuntu and Windows

Orpheus is a single self-contained Rust program (the binary is called `latte`). It has **zero
external crate dependencies**, so it builds offline with nothing but a Rust toolchain — no
package downloads, no network. This guide covers both Ubuntu/Linux and Windows.

The same source tree builds on both platforms; only the toolchain installation and a few path
conventions differ.

---

## What you need

- **Rust 1.75 or newer** (`rustc` and `cargo`). That is the only build requirement.
- **`rustc` available at run time, too** — but only if you want the *optimizing compiler*
  (Anvil) to be the execution engine. Anvil compiles each evaluated expression to native code
  and runs it; the very first time it needs `rustc` on your `PATH`. If `rustc` is not present at
  run time, Orpheus automatically falls back to its built-in interpreter, so the system still
  works — it just interprets instead of compiling. (You can also force the interpreter with
  `latte eval --interp …`.)

There is nothing else to install: no databases, no JS toolchain, no web server.

---

## Ubuntu / Linux

### 1. Install Rust

Either use the distribution packages:

```sh
sudo apt update
sudo apt install -y rustc cargo
```

or, for a newer toolchain, use rustup:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Check the version:

```sh
rustc --version    # expect 1.75 or newer
```

### 2. Build

From the source directory (the one containing `Cargo.toml`):

```sh
cargo build --offline --release
```

`--offline` is safe because there are no dependencies to fetch; you can drop it if you prefer.
The optimized binary lands at `target/release/latte`. Put it on your `PATH` if you like:

```sh
install -Dm755 target/release/latte ~/.local/bin/latte
```

### 3. Run

```sh
./target/release/latte eval "(add 2 3)"          # → 5  (compiled natively by Anvil)
./target/release/latte eval "(mul (add 3 4) 5)"  # → 35
./target/release/latte cli                       # interactive command-line console
./target/release/latte game chess                # two machines play a full game
./target/release/latte icomb                     # interaction-combinator reductions
./target/release/latte rustc "(add 2 3)" --run   # compile to native Rust and run
./target/release/latte gui                        # the web GUI on http://127.0.0.1:8088/
```

The GUI listens on `127.0.0.1:8088` by default; change it with `--listen ADDR`, choose the page
root with `--root DIR`, and use `--no-open` to skip launching a browser (useful on a headless
server). In a desktop session the bundled `install-gnome.sh` adds an application-menu entry.

### Running from the shipped distribution

The distribution archive (`orpheus-linux-x86_64.tar.gz`) already contains a prebuilt `latte`
plus the full source. Unpack it and either run the binary directly or rebuild it in place:

```sh
tar xzf orpheus-linux-x86_64.tar.gz
cd orpheus-linux-x86_64
./latte eval "(mul 6 7)"            # use the prebuilt binary
cargo build --offline --release     # or rebuild from the included source
```

---

## Windows

### 1. Install Rust

Download and run **rustup-init.exe** from <https://rustup.rs>. Accept the defaults; rustup
installs `rustc`, `cargo`, and (with the MSVC default) the needed Microsoft C++ build tools. If
prompted, install the "Desktop development with C++" Visual Studio Build Tools, or choose the
GNU toolchain in the rustup installer to avoid that.

Open a fresh **PowerShell** or **Command Prompt** and check:

```powershell
rustc --version    # expect 1.75 or newer
```

### 2. Build

From the source directory:

```powershell
cargo build --offline --release
```

The binary is `target\release\latte.exe`.

### 3. Run

```powershell
.\target\release\latte.exe eval "(add 2 3)"
.\target\release\latte.exe cli
.\target\release\latte.exe game chess
.\target\release\latte.exe gui
```

The GUI serves on `http://127.0.0.1:8088/`; open it in a browser (Windows may not auto-open it,
so navigate there manually, or pass `--no-open`). Quote expressions with double quotes in both
PowerShell and Command Prompt. If Windows Defender Firewall prompts when you start the GUI, you
only need to allow local (loopback) access.

On Windows, Anvil compiles to `latte`-built `.exe` programs and caches them under
`%LOCALAPPDATA%\orpheus\anvil`. Everything else behaves exactly as on Linux.

---

## Compiled-program cache (both platforms)

When Anvil is the engine, each distinct expression is compiled **once** and the resulting native
binary is cached, keyed by a hash of the generated source. Running the same code again reuses the
cached binary — no recompilation — and a warm run is as fast as the interpreter. Recompilation
happens **only** when the code actually changes (the expression itself, or a library function it
reaches).

- **Cache location**: `~/.cache/orpheus/anvil` on Linux, `%LOCALAPPDATA%\orpheus\anvil` on
  Windows (override with the `ORPHEUS_CACHE` environment variable).
- **Inspect / clear**: `latte cache path` shows the directory and how many programs are cached;
  `latte cache clear` empties it.
- **Force a rebuild**: `latte eval --rebuild "<expr>"` ignores the cache for that run.

---

## Testing

The full test suite (including the differential tests that compile generated programs with
`rustc` and check them against the interpreter) runs with:

```sh
cargo test --offline
```

---

## Troubleshooting

- **`rustc` not found at run time** — evaluation silently falls back to the interpreter, so
  results are still correct, just interpreted. Install Rust (above) to enable native compilation,
  or use `latte eval --interp` to interpret on purpose.
- **GUI port already in use** — start it on another port: `latte gui --listen 127.0.0.1:9000`.
- **Headless / server use** — add `--no-open` so it does not try to launch a browser.
- **A stale result after editing a library** — you should never see one (the cache key changes
  when reachable code changes), but `latte cache clear` or `--rebuild` forces a fresh compile.
- **Linker errors on Windows** — install the Visual Studio C++ Build Tools, or reinstall rustup
  selecting the GNU toolchain.
