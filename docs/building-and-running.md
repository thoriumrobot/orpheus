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

## The command map (what shipped where)

A quick tour of the tools — each also has a GUI surface in the System (`latte serve`, then
middle-click commands in `System.Tool`):

```sh
# language
latte eval "(fib 30)"                 # Anvil native compilation (the default engine)
latte eval --interp "<expr>"          # the Loom interpreter
latte eval --net "<expr>"             # the interaction-net engine (audited)
latte net  "let gcd = fn [a b] -> if (b == 0) then a else (gcd b (mod a b)) in (gcd 1071 462)"
latte net --peano "(mul 12 11)"       # unary Peano mode (the pedagogical engine)
latte net --par 4 "<expr>"            # the batch-claimed PARALLEL reducer (a verified demo)
latte repl                            # interactive

# market tools — all run on REAL data; --live refreshes it from Coin Metrics
latte fetch                           # refresh + cache the BTC daily close series
latte fetch --market eth              # any Coin Metrics market (eth ltc xrp ada doge sol …)
latte fetch --all                     # all the curated markets at once
latte fetch --news <url>              # curl a document into the news/ ADVICE STREAM
latte ta [--live] [--win N] [--market SYM]    # technical analysis (lib/ta.lat, on Loom)
latte trade [--live] [--news FILE] [--market SYM] [--account N] [--kelly F] [--sentiment S]
                                      # the advisor; --market trains + registry-caches that
                                      # market's volatility model (HONEST edge-vs-baseline);
                                      # documents in news/ are scored whole and blended in
latte sentiment "<headline text>"     # trained classifier + LM lexicon + fused score
latte sentiment --file report.txt     # score a whole DOCUMENT, sentence by sentence
latte sentiment --doc <name>          # score docs/<name>.md the same way
latte chart market [--live] [--days N] > market.svg     # real dates on the axis
latte chart bar 3 1 4 1 5             # bar | line | scatter (layout in lib/plot.lat)

# models and graphics
latte nn [--epochs N]                 # autodiff-trained stacks + transformer + SSM blocks
latte fin [--direction] [--iters N]   # the financial-ML pipeline (+ Sharpe/maxDD/turnover
                                      #   net of 5bp costs on the direction task)
latte plan [--demo3 | --spec FILE]    # Towards a New Socialism: values, gross outputs,
                                      #   market steering, harmony balancing (docs/planning.md)
latte gfx > scene.svg                 # the gfx demo scene (lib/gfx.lat)
latte trace --w 160 --h 120 > rt.svg  # the RAY TRACER written in Latte (Anvil-compiled)

# the conlang engine
latte sca --file lib/pie.sca dʰeh₁s mr̥to     # PIE -> Solar Speech
latte evolve saules bazdā                     # Solar -> Heart Speech
latte sca kasa k>g s>z/a_a                    # ad-hoc rules

# the system
latte serve                           # the Oberon-style GUI (texts with embedded
                                      # objects, parallel tools — docs/the-system.md)
latte fmt <file.lat> [--write]        # the conservative, compile-checked formatter
latte pkg                             # list system libraries and user packages
latte debug [--break ARM] "<expr>"    # the Loom call tracer: every arm call as a tree
latte trace [--scene FILE|EXPR] [--w N] [--h N]   # ray-trace any Latte scene

# the conlang suite (also /soundlib and /phono in the GUI)
#   the sound-change library assembles attested changes into a SCArs file and
#   runs it; the phonology builder generates words and evolves them through it
curl -s -X POST localhost:8088/api/soundlib -d 'changes=grimm1 verner
words=pater bhrater'
curl -s -X POST localhost:8088/api/phono -d 'preset=pie
n=12
changes=grimm1 apocope'
```

In the GUI: `/` is the System — **texts with embedded objects** (run a `chart`/`trace`/`trade`
line inside a text and the output embeds right there; texts Store to `text/*.md` with their
objects and rehydrate on load; the header links open editable TOOL TEXTS; see
**docs/the-system.md** for the full tour), `/docs` is
the manual shelf (every document editable in place; **＋ New** creates one, with live
`` ```chart `` figures), `/editor` is the WYSIWYG page editor (find & replace, Ctrl/⌘+S, status
bar), `/derive` derives Ligurian live (PIE → Solar → Heart, plus the vocabulary generator), and
`/trade`, `/fin`, `/chart`, `/gpu`, `/chess`, `/grammar`, `/plan` are the tool pages.

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
