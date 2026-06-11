# Orpheus

**Orpheus** is a minimalist functional operating environment built from scratch with no
external crates. Its layers:

| Component | Name | What it is |
|-----------|------|------------|
| System | **Orpheus** | the whole environment |
| Core VM | **Loom** | a 12-rule axiomatic virtual machine |
| Datatype | **Knot** | the single universal datatype (atoms + cells) |
| Language | **Latte** | a functional language with closures, modules & a standard library |
| Markup | **Facet** | a markup language whose holes call environment tools |
| Server | **Hymn** | a web server hosting Facet pages (static assets + fonts) |
| Sound-change applier | **SCArs** | a fully-featured conlang sound-change engine, written in Latte |

It keeps shared state across machines over the Internet (content-addressed event log,
gossip, no blockchain), persists durably with safe upgrades, accelerates with audited
jets, and ships a working application: a sound-change applier that a hosted web page
calls to generate conlang vocabulary on the fly. Every release includes **Ubuntu and
Windows binaries**.

## Build, test, run

```sh
cargo build --release          # ./target/release/latte  (zero dependencies)
cargo test                     # 183 unit tests across all layers
./latte eval "(mul 6 7)"     # 42   — standard library is linked automatically
./latte evolve serdā nūn     # SCArs: heardō   nū̃
./latte serve                # Hymn hosts ./lib/site at 127.0.0.1:8080
```

## Latte is a fully-featured language

The core is tiny — the only arithmetic primitive Loom provides is `+` (successor). On top
of that Latte has literals, tags, faces, cells, `head`/`tail`, `==`, `if`, `let`,
`case`, `loop`/`again`, `core` modules, Lisp-style `(f x)` calls, `fn` closures and
higher-order functions, `fast` jet hints, and positioned errors. The remaining "fully
featured" surface comes from two things added in this phase:

**A module / import system.** Several module sources link into one shared namespace; a
user module can shadow library names. `import std` at the top of a module pulls in the
standard library; bare `eval` links it automatically.

```
import std
core mymath
  double = fn [n] -> (mul n 2)
  big    = fn [_] -> (gt (double 30) 50)   :: uses std mul + gt
end
```

**A standard library** (`lib/std.lat`), written in Latte itself and built up from `+`:

- arithmetic — `dec add sub mul div mod`
- comparison & logic — `lt gt lte gte and or not min max`
- lists (cons cells, `0`-terminated) — `len reverse append nth member range`
- higher-order — `map filter foldl`

```sh
./latte eval "(div 84 2)"          # 42
./latte eval "(max 17 42)"         # 42
./latte eval "(len [ 1 [ 2 [ 3 0 ] ] ])"   # 3
```

(Arithmetic on naturals is unary-cost; the hot operations are jettable, like `add`.)

## Molds & auras — a type system on Loom

`import mold` adds a small **type system**, written in Latte itself. A *mold* is a
noun describing a shape; an *aura* labels an atom for display.

```
[0 aura]      atom mold (aura = a %term: ud ux ub t tas …)
[1 [mh mt]]   cell mold        [2 me]   list mold        [3 [ma mb]]   fork mold
```

Three operations run on Loom over those descriptors:

- `bunt m` — the default value of a mold
- `nest m n` — does noun `n` fit mold `m`? (loobean)
- `clam m n` — coerce `n` into mold `m` (total: never crashes; mismatches fall back to bunt)

Auras drive how atoms print: the same atom `255` shows as `255` (`@ud`), `0xff` (`@ux`),
`0b11111111` (`@ub`), or as text/term for cords (`@t`/`@tas`). The lexer now reads
`0x…`/`0b…` literals, and `iscell` exposes Loom's cell test.

```sh
./latte mold        # a guided tour
# mold [@ud @ux]:  bunt = [0 0x0]   clam 7 = [0 0x0]   clam [9 250] = [9 0xfa]
# mold (list @ud):  clam [1 [2 [3 0]]] = ~[1 2 3]
```

This is a *runtime* mold system (validation, defaults, coercion, aura display), not a
static type checker — fitting for Loom's untyped noun substrate.

## Static type checking

`latte typecheck <expr>` runs a compile-time companion to the runtime molds. It infers
a structural type over a small lattice that mirrors nouns — `@` (atom), `[T T]` (cell),
and `*` (a noun of unknown shape, the top type) — and reports an error only when an
operation is *provably* misapplied:

```sh
latte typecheck "[1 [2 3]]"                    # [1 [2 3]] : [@ [@ @]]
latte typecheck "head [1 2]"                   # head [1 2] : @
latte typecheck "head 5"                       # type error: `head` of an atom
latte typecheck "+([1 2])"                     # type error: `+` expects an atom but got a cell
latte typecheck "let x = 5 in head x"          # type error (the binding is an atom)
latte typecheck "(add 1 2)"                    # (add 1 2) : *
```

The checker is sound but conservative: anything it cannot pin down — arm calls, loops,
closures — becomes `*`, which is compatible with every operation, so a working program is
never rejected, while argument errors are still caught inside an otherwise-`*` call. This
is the honest static fragment over Loom's untyped substrate (molds remain the runtime
enforcement; the checker adds an early, no-false-positive warning layer).

## SCArs is a fully-featured sound-change applier

The matching engine (`lib/sca.lat`) runs on Loom; the host compiles rules down to it.
Rule syntax is `FROM > TO / PRE _ POST` (tokens are whitespace-separated; omit `/…` for
no context). Supported:

- **multi-segment** FROM and TO — clusters, endings, **metathesis** (`s p > p s`)
- **deletion** (empty TO) and **insertion** (empty FROM: `> a / k _ t`)
- **multi-segment context** on either side, with the word **boundary** `#`
- **wildcard** `*` — any one segment
- **multi-character graphemes** via longest-match tokenization
- **phoneme categories** (named `class`es) and **category correspondence**:
  `STOPV > STOPVD` maps `p→b, t→d, k→g` by index (`apataka → abadaga`)
- ordered rule application

```sh
./latte sca atta "t t > s t"                 # asta
./latte sca aspa "s p > p s"                 # apsa  (metathesis)
./latte sca akt " > a / k _ t"               # akat  (epenthesis)
./latte sca apataka "class STOPV = p t k" "class STOPVD = b d g" \
                      "class V = a e i o u" "STOPV > STOPVD / V _ V"   # abadaga
./sca_demo.sh                                  # full tour
```

On top of the segmental engine, SCArs adds two **prosodic** passes that need syllable and
stress structure (so they run as a post-pass): **breaking** (stressed penult `a/e/o` →
`ao/ea/uo`) and **nasalization** (vowel + lost coda nasal; long vowels take a combining
tilde). So `zelā→zealō`, `serdā→heardō`, `bendā→mẽdō`, `kamtón→gãtõ`, `nūn→nū̃`.

The **rule language itself** also expresses stress- and cluster-conditioned changes directly:
stress is written with an acute accent (`á é í ó ú`) and a multi-segment context distinguishes
open from closed syllables, so `á > a o / _ C V` breaks `kása→kaosa` but leaves the closed
`kásta` untouched. A worked ruleset ships as `lib/breaking.sca`, run with
`latte sca --file lib/breaking.sca kása kásta` (see `docs/scars-sound-changes.md`).

## The hosted Ligurian page (Hymn + Facet + SCArs + fonts)

`lib/site/index.facet` is served by Hymn at `/`. **No Heart Speech word is written into
the page** — the title, a 20-word dictionary, a nasal-vowel showcase, and a line of the
Hymn to Wine are all produced at request time by `SCArs.evolve`. Hymn serves the bundled
subset **Ligos** font (mark-positioning preserved) over `@font-face`, with correct MIME
types and exact bytes, so stacked diacritics like `nū̃` and `ō̃` (macron + combining
tilde) render.

Hymn is a fully-featured HTTP/1.1 server: persistent **keep-alive** connections, header
parsing, conditional GET (a content **`ETag`** via SHA3 + `If-None-Match` → `304`, plus
`Last-Modified`/`If-Modified-Since`), byte **`Range`** requests (`206`/`416`,
`Accept-Ranges`), `Date`/`Last-Modified` headers, `HEAD`, access logging, read timeouts,
and `400`/`404`/`405` handling. Rendering is pure and deterministic with no shared mutable
state, so concurrent requests use SCArs safely.

**The live derivation page.** `/derive` derives **both registers in real time**: PIE →
Solar Speech runs `lib/pie.sca` (the grammar's §II table, stages 0a–9 — laryngeal
coloring, syllabic resonants, satemization, the labiovelar split, all verified against
the grammar's own examples: `dʰeh₁s→dēs`, `mr̥to→marto`, `gʷih₃w→bīw`, `sāwel→saul`),
and Solar → Heart runs `lib/ligurian.sca` plus the prosodic passes. A **vocabulary
generator** (`lib/lexis.lat`, written in Latte) builds phonotactically valid Solar roots
from a seeded stream and derives each one's Heart form on the spot. The grammar page now
links there instead of to an external script.

## Distribution — Ubuntu & Windows binaries

Each release ships binaries for **Ubuntu/Linux x86-64** and **Windows x86-64**. Orpheus
has zero external crates and no platform-specific code, so the same sources build on both.

- `.github/workflows/release.yml` — a two-OS CI matrix (`ubuntu-latest`, `windows-latest`)
  that builds + tests on every `v*` tag and attaches both archives to the release.
- `scripts/build-all.sh` — builds the native Linux binary and cross-compiles the Windows
  binary (rustup target + `mingw-w64`) into `dist/`.
- `dist/orpheus-linux-x86_64/` — a real, runnable Ubuntu build (binary + `lib/site`),
  packaged with `SHA256SUMS`.

See `DISTRIBUTION.md`. (The Windows `.exe` is produced by that pipeline; this build
environment has the mingw cross-linker but cannot fetch the Windows `std`.)

## Connecting machines, persistence, jets

Each node keeps an append-only, content-addressed event log; state is the deterministic
fold of an agent over the total order `(lamport, node_id, hash)`. Nodes sync by gossip +
anti-entropy over TCP — convergence without consensus, with late-join catch-up,
restart-and-rejoin from the durable log, and dead-peer pruning. Upgrading an agent
re-folds the log instead of corrupting state. **Log compaction/GC** (`--compact-every N`)
folds the event log into a durable *baseline* (state + watermark) and truncates it, bounding
disk and memory; a node that has GC'd its log still bootstraps a fresh peer by shipping the
baseline (a `SNAP` frame), so the peer converges without the archived events. Jets give
native acceleration that is **audited** against the pure interpreter.

```sh
./demo.sh ; ./kv_demo.sh ; ./internet_demo.sh ; ./persist_demo.sh ; ./sca_demo.sh
```

## Mocha — the application environment

Mocha runs higher-level **apps written in Latte** on the same persistent, distributed
runtime as everything else. An app is a `core` module exposing two arms:

```
poke = fn [action state] -> [effects state]   :: a state transition
peek = fn [query  state] -> value             :: a read-only view
```

Because each poke is a durable, gossiped event, an app inherits persistence,
strong-eventual-consistency convergence, time-travel, and log compaction for free — the
Rust host is only a thin shell for command parsing, I/O, and the bridge to SCArs. Three
apps ship (`lib/todo.lat`, `lib/lexicon.lat`, `lib/forge.lat`), built on the `lib/mocha.lat`
support library:

```sh
latte mocha                                   # a guided tour of both apps
latte mocha --app todo --store /tmp/todo \
        --poke "add buy milk" --poke "add ship orpheus" --peek count --peek list
# two machines converging on one app:
latte mocha --app todo --id 1 --listen HOST:PORT --poke "add from A" --run-secs 6
latte mocha --app todo --id 2 --listen HOST:PORT --peer A_HOST:A_PORT \
        --poke "add from B" --peek list          # sees both tasks
```

The `lexicon` app is a Solar→Heart dictionary whose Heart forms are derived by **SCArs**
at poke time and then stored and served by the Latte app — e.g. `add ligā` stores
`ligā → liɣō`. This is the intended shape of the system: primitives in Rust, the
environment and its tools in Latte.

## The self-hosting environment

`latte repl` is an interactive Latte session. Definitions entered at the prompt
accumulate into a live `core` that is recompiled and run on Loom, so the environment
hosts evolving Latte code at runtime — later expressions call arms you defined earlier,
plus the std / mold / plan libraries. It also fronts the rest of the toolbox.

```
» double = fn [x] -> (add x x)
defined double
» (double 21)
42
» (values [400 [300 0]] [ [200 [100 0]] [ [500 [100 0]] 0 ] ] 40)
[581 [656 0]]
» :t head [1 2]
head [1 2] : @
» :sca ligā
ligā → liɣō
```

## Team coding across connected machines

`latte team` is collaborative coding built on the **Forge** Mocha app: a shared,
append-only log of `[author snippet]` entries. Because each share is a durable, gossiped
event, every teammate's node converges on the same codebase — across the room or across
the Internet.

```sh
latte team --as alice --listen HOST:PORT --share "double = fn [x] -> (add x x)" --run-secs 6
latte team --as bob   --listen HOST:PORT --peer ALICE_HOST:PORT \
        --share "square = fn [x] -> (mul x x)" --show     # sees both authors' snippets
```

## Planning — *Towards a New Socialism*

`latte plan` runs Cockshott & Cottrell's planning algorithms with every numeric
step in `lib/plan.lat` on Loom: iterative **labour values** (v = l + vA),
**Leontief gross outputs** (x = y + Ax), **labour-token accounting** (Σ v·y),
**consumer-goods market clearing** (price/labour-value ratios steer the next
period's targets, TNS ch. 8), and **harmony-function balancing** (the concave
social-utility maximization by marginal-equalizing transfers, TNS pp. 94-99)
when the labour budget binds. `--demo3` shows the whole pipeline;
`--spec FILE` plans **your** economy from a five-line spec (`sector`/`demand`/
`market`/`labour`); the `/plan` page has a form, and `plan demo3` embeds the
report in any GUI text. `docs/planning.md` documents the algorithms *and* the
calculation-debate criticisms (Hayek's information argument, Shalizi's
"Jacobi solvers" critique, and the replies) — a contested proposal, presented
with its contest.

## Numeric libraries — signed numbers, tensors, and machine learning

Latte atoms are naturals, so the numeric stack starts by building **signed fixed-point
numbers** in Latte: `lib/num.lat` represents a number as a cell `[sign magnitude]` (sign 0
or 1, magnitude scaled ×1000) with `nadd`/`nsub`/`nmul`/`ndiv`/`nlt`. The arithmetic rides
the jetted std ops, so it is fast.

On top of that, `lib/tensor.lat` provides **n-dimensional tensors**: a tensor is
`[shape data]` with a dimension list and row-major elements. It has shape/size/reshape,
row-major indexing (`tget`), reductions (`tsum`, `tdot`), elementwise maps (`tadd`, `tsub`,
`thad`, `tscale`), and 2-D matrix multiply (`tmatmul`).

```sh
latte tensor      # dot product = 32, 2x2 matmul = [19 22 43 50], shapes, indexing
```

Finally `lib/ml.lat` **trains a model by gradient descent**: linear regression
`y = w·x + b` fit over the signed-number tensors. `latte ml` fits `y = 2x + 1` from four
points and recovers `w ≈ 2.01, b ≈ 0.97` (the 3-decimal fixed point sets a floor on how
close the descent can creep before the `lr·gradient` update underflows).

```sh
latte ml --iters 5000     # learned w = 2.011, learned b = 0.968
```

`lib/ml.lat` also carries three further model families, all in Latte over the signed-number
library: a **perceptron** (`latte ml perceptron`, an online linear classifier — learns `1 1 0 0`
on a separable set), **k-means** clustering (`latte ml kmeans` — recovers centroids ≈ 2.0 and
11.0), and **k-NN** (`latte ml knn` — labels a query by its nearest training point).

**Neural networks** live in `lib/nn.lat`, where a network is *composable data* — a list of tagged
layers (`%dense`, `%relu`, `%tanh`, and `%res` for a residual/skip connection) that `net_fwd`
folds into a forward pass. A `resblock` is just a `%res` layer wrapping two dense layers, so
ResNet-style nets are built by consing more blocks onto the list. Beyond inference, a
one-hidden-layer MLP trains by **full backpropagation** (ReLU hidden, linear output, MSE).
`latte nn` learns `y = |x|` — impossible for a linear model, easy with one ReLU layer — driving
the loss from ≈ 2.7 to ≈ 0 (predictions 3.0/1.0/1.0/3.0) and writing a loss-curve SVG.

**Practical financial ML** lives in `lib/fin.lat`, a small leak-aware pipeline after Lopez de
Prado: momentum + realized-volatility features, **train-only** standardization, a **walk-forward
split** (never shuffled), and a logistic-regression classifier — composing `num`, `nn`, and the
chart renderer. After finding price-only models near-chance on gold, the demo was moved to the
market where momentum and volatility clustering are strongest — **Bitcoin** (research: Moskowitz/
Ooi/Pedersen 2012; Shen/Urquhart/Wang 2022; Katsiampa 2017). `latte fin` trains on **1300 real
daily BTC/USD closes (2022–2026)** embedded in the binary. Daily *direction* stays near-random
(de Prado's point), but the default **next-day volatility-regime** task reaches ≈ 55% vs a ≈ 51%
baseline — a genuine **+3–4 point out-of-sample edge** — and the same model earns a +50-point edge
on a synthetic mean-reverting series. It writes a variance-timing equity curve (`fin-equity.svg`),
runs live on the `/fin` GUI page, and honestly reports its numbers — a working pipeline, not a
profit machine. Flags: `--vol` (default), `--dir`, `--horizon N`, `--iters N`.

A **graphics library** lives in `lib/gfx.lat`: a scene is a list of tagged shapes
(`%line`/`%rect`/`%circle`/`%poly`/`%text`) over packed-RGB colours, built in Latte and rendered to
SVG by the host (`src/gfx.rs`). `latte gfx` draws a demo scene; `POST /api/gfx` renders it in the GUI.

A **data-parallel GPU compute library** lives in `lib/gpu.lat`: buffers and kernels (`map`,
`zipWith`, `reduce`, `saxpy`, `dot`, `matmul`, per-pixel `shade`) expressed as data. The target
device is an **NVIDIA GeForce RTX 4070 Ti SUPER** (16 GB — the card's real VRAM — 8448 CUDA cores,
66 SMs). The kernel/buffer model matches what a CUDA backend would use, so the card is a drop-in
backend swap; in this zero-dependency, no-CUDA build the active backend is a genuine multi-core CPU
backend (`std::thread`). It integrates with `nn`/`ml` (matmul is the dense-layer kernel) and `gfx`
(a Mandelbrot shader renders through the graphics path). `latte gpu` shows the device, benchmarks
matmul (parallel vs serial, results checked equal), runs a Latte `gpu` program on the backend, and
writes a Mandelbrot image; the `/gpu` GUI page shows all of it live via `POST /api/gpu`.

For **data visualization**, `lib/plot.lat` computes chart *layout* in Latte — scaling data
to pixel geometry — and Hymn serializes it to SVG. Bar, line, and scatter charts are
available from the CLI, the `/chart` GUI page, or `POST /api/plot`:

```sh
latte chart bar  3 1 4 1 5 9 2 6  > chart.svg
latte chart line 1 2 3 5 8 13
```

The market tools run on **real, refreshable data**: `latte fetch` pulls the live Coin
Metrics BTC/USD series (cached; the embedded press-anchored recent days splice on top),
`latte ta [--live]` computes five classical indicators **in Latte** (`lib/ta.lat`: SMA,
EMA, ROC, RSI, MACD, Bollinger %B) with a composite vote, `latte chart market --live`
charts the series with SMA overlays, and `latte trade [--live] [--news FILE]` fuses the
TA composite (60%) with Loughran-McDonald **news sentiment** over real dated headlines
(40%), sized by fractional Kelly × volatility targeting. `lib/nn.lat` now spans the
modern architectures — sigmoid/softmax/layernorm/conv1d, single-head **self-attention**,
and seeded **transformer blocks** — all as composable data. Charts embed into GUI-written
reports as ```` ```chart ```` blocks.

A full walkthrough — signed numbers, tensors, drawing charts, and designing/training your
own model (including the fixed-point precision floor and the stable learning-rate range) —
is in [`docs/visualization-and-ml.md`](docs/visualization-and-ml.md).

## The GUI — an Oberon-class system surface

`latte serve`, open `/`. The display is tracks of **viewers** (title bar + menu
+ frame, all drag-resizable), and the medium is the **text**: middle-click any
command line, anywhere, and it runs. Inside a text frame, object-producing
commands (`chart`, `gfx`, `trace`, `ta`, `trade`, `fin`, `gpu`, `derive`,
`plan`) embed their output **into the text under the line you ran** — Oberon's
texts-with-elements, live. Texts **Store to `text/*.md` with their objects**
(serialized as ```` ```tool <command> ```` fences) and rehydrate on load by
re-running each command. The header links open editable **tool texts**
(Trade.Tool, Plan.Tool, …) rather than navigating away; tools run **in
parallel** (one server thread per request), each filling its own viewer or its
own spot in your text while you keep editing. Modules compile from frames into
the running system (Compile), persist as **user packages** in `pkg/` (Store),
and load at every startup; Format runs the compile-checked source formatter.
The full tour: `docs/the-system.md`. The toolbox around it: **Draw**, a real
vector-graphics editor whose stored drawings embed in texts (`drawing <name>`);
a **debugger** (`latte debug`, or `debug <expr>` in any text) — the Loom call
tracer rendering every arm call as an expandable tree with `break=ARM`
breakpoints; the **conlang suite** (an attested sound-change library that
assembles SCArs rulesets, and a phonology builder that generates and evolves
vocabularies through it); **xiangqi** on a traditional board (river, palace
diagonals) against a model whose piece values were learned by gradient descent
in Latte, driving a native search — whose pseudo-move optimization transferred
back to make the chess engine a ply deeper; and a ray tracer whose scene is
Latte data (`trace … scene=[ … ]`), with key + fill lights and graded
reflections. The conlang suite checks generated phonologies against the
typological record (UPSID/WALS implications, with evidence), and persists
rulesets as ordinary `.sca` files plus phonologies as `.phon` files that any
tool can parse; the Draw editor designs covers and posters (canvas presets,
backgrounds, font control, stars, snap, centering). Tool texts and System.Tool
persist your stored versions across runs, and every command's output — your
own packages' arms included — embeds as an object in texts, so **new tools are
creatable wholly inside the GUI** (the recipe is in docs/the-system.md).

## The GUI — a WYSIWYG Facet editor

`latte gui` serves a browser GUI through Hymn: a **what-you-see-is-what-you-get editor**
for Facet documents. You type Facet source on the left and see it rendered live on the
right, because the page POSTs to a small dynamic API that Hymn now exposes:

- `POST /api/render` — render submitted Facet source to HTML (the live preview; errors
  shown inline)
- `POST /api/save` / `GET /api/load` — persist and recover the in-memory document
- `GET /api/files`, `GET`/`POST /api/file?path=NAME.facet` — list, open, and **edit the
  actual Facet page files** in the site root (path-checked to stay inside the root)
- `POST /api/run` — the Oberon-style command runner (below)
- `GET /api/sources`, `GET`/`POST /api/source?name=NAME` — **list, open, and recompile the
  system's own Latte modules** in place (the Oberon edit→compile→run loop on real source)
- `POST /api/compile` — compile a `core NAME …` module into the running system

The editor is a full editing surface: **find & replace** (Ctrl/⌘+F, replace-one/all with a
live match count), **Ctrl/⌘+S** saves, a status bar (line:column, character and word counts,
a modified marker), font-size controls, Tab inserting spaces — and the live preview renders
markdown including embedded ```` ```chart ```` figures.

The document model is itself a Latte app — `lib/editor.lat`, a Mocha app whose state is
the document — so a saved document is durable and can sync across machines just like any
other Mocha app. The browser side is a thin, dependency-free page (`lib/site/editor.html`);
all rendering runs through Facet on Loom, and all state lives in Latte. The editor opens
and saves the hosted `*.facet` pages and is fully **Unicode-safe** — Heart Speech glyphs
like `ɣ`, `ā`, `ē` round-trip through edit, save, and render byte-for-byte.

### Oberon-style System GUI

`/` (and `/system`) is a tiled, text-centric command environment built in the spirit of the
Oberon system. The screen is split into two vertical **tracks** of **viewers** that tile and
never overlap; each viewer has a one-line **menu** of executable commands and a body.

Every body is an Oberon-style **text frame**: editable text that is *also* the command
interface. **Text is the user interface** — point at any `Module.command args` line (in a
tool text, a source frame, even the Log) and **middle-click** to execute it; or put the caret
on it and press **Ctrl/⌘+Enter**; or **select** any command text and run that. The frame
under the pointer is located by line, so the same gesture works everywhere. Output
accumulates in the **System.Log** (itself a text frame — you can re-run a line from it), and a
**Modules** viewer lists the loaded modules (click one to open its source).

Commands fall into three families:

- **`System.*`** — the display/OS commands, handled in the browser: `System.Open NAME`
  (open a module's source in a new frame), `System.Close`, `System.Grow`, `System.Copy`,
  `System.Clear`, `System.New NAME`, `System.Modules`, `System.Chess` (open the board). A
  viewer can be **marked** (click it; it shows `✷`); commands with `*` act on the marked one.
- **`Compiler.Compile *` / `Compiler.Store *`** — compile the marked source frame into the
  running system (`/api/compile`), or compile **and persist** it to its `lib/NAME.lat` file
  (`/api/source`). No binary rebuild — the new definitions are live immediately.
- **everything else** → `/api/run`: `Module.command args` (calls a Latte arm), plus the tools
  surfaced as verbs — `eval`, `type`, `libs`, `sca`/`evolve` (Ligurian Solar→Heart), **`scar`
  (the general SCArs rule engine, `scar kasa k>g s>z/a_a → gaza`)**, **`plan` (the economic
  planner report)**, and `icomb`. The command set is itself Latte source you can open and edit.

The command set ships as Latte in **`lib/tool.lat`** (`core tool`): `Tool.fib`, `Tool.fact`,
`Tool.gcd`, `Tool.primes`, `Tool.collatz`, `Tool.greet`, … The default desktop has a
**System.Tool** text, an editable **hello.Mod** source frame, the **System.Log**, and the
**Modules** list — so the edit→compile→run loop is a gesture away: mark `hello.Mod`, run
**Compile**, then middle-click `hello.fib 20`.

### Chess (graphical game frontend)

`/chess` is an interactive board (rules and the learned opponent are Latte: `lib/chess.lat`,
`lib/chessml.lat`). You can play **against the model** (greedy, or the gradient-descent-learned
*Minerva* evaluator), **local two-player** on one board, or **against another user on a
connected machine**. The game runs as a Mocha app (`lib/chessgame.lat`) on a gossip Node, so
each move is a durable, replicated event: two GUI servers that peer with one another share one
converging board. Illegal pokes are no-ops, so the shared game can never be corrupted.

```sh
# play locally / vs the model:
latte gui --listen 127.0.0.1:8088                        # open http://127.0.0.1:8088/chess
# two connected machines sharing a game (open /chess on each, pick opposite colours):
latte gui --listen :8088 --chess-listen 127.0.0.1:9601 --peer 127.0.0.1:9602   # machine A
latte gui --listen :8089 --chess-listen 127.0.0.1:9602 --peer 127.0.0.1:9601   # machine B
```

The board talks to `POST /api/chess` (`state` | `new` | `move FROM TO` | `ai greedy|ml`),
which reads the gossiped position from the node and computes legality, status, and the model's
replies on the fast unbounded engine.

```sh
latte gui --listen 127.0.0.1:8088 --store /tmp/editor    # open http://127.0.0.1:8088/
```

This is the system's GUI: there is no native windowing toolkit (Orpheus is dependency-free
and the web stack via Hymn is the display layer), so the GUI is delivered in the browser, but
the *logic* — the command set, the modules, the compiler loop, the chess rules — lives in
Latte on the Loom.

## What's implemented vs. the full spec

- Loom: 12 rules, slot/edit/peg, jam/cue, SHA3 CIDs, fuel-bounded crashes, audited jets.
- Latte: full expression language, `core` modules, `fn` closures + higher-order fns,
  **module/import linking**, a **standard library** (`lib/std.lat`), a **mold/aura type
  system** (`lib/mold.lat`: bunt/nest/clam + aura display), and a **static type checker**
  (`latte typecheck`).
- SCArs: multi-segment rules, categories + correspondence, contexts, boundaries,
  wildcard, insertion/deletion, metathesis, graphemes, + prosodic breaking/nasalization.
- Facet markup + the **fully-featured Hymn HTTP/1.1 server** (keep-alive, ETag/304,
  Range/206, conditional GET, fonts), hosting `lib/site`.
- Distributed runtime, persistence, safe upgrades, **log compaction/GC with baseline
  transfer**, jets; Ubuntu + Windows release pipeline.
- The **Mocha** application environment: Latte apps (`poke`/`peek`) on the persistent,
  distributed runtime, with SCArs bridged in (`lib/mocha.lat`; `todo`, `lexicon`, `forge`).
- A **self-hosting REPL** (`latte repl`), **team coding across machines** (`latte team`,
  the Forge app), and **planning calculations** from *Towards a New Socialism*
  (`latte plan`, `lib/plan.lat`) — all higher-level work written in Latte.
- Native **jets** for the standard library's arithmetic, audited against the pure reduction.
- A browser **GUI** served by Hymn — a **WYSIWYG Facet editor** with live preview and a
  durable, syncable document model written in Latte (`lib/editor.lat`, `lib/site/editor.html`).
- Facet **conditionals** (`{{if}}/{{else}}/{{end}}`) alongside the existing `{{each}}` loops.
- **Numeric libraries in Latte**: signed fixed-point numbers (`lib/num.lat`),
  **n-dimensional tensors** (`lib/tensor.lat`), and **gradient-descent ML training**
  (`lib/ml.lat`) — with `lt`/`dec` added to the jetted ops to keep them fast.
- The browser GUI now **edits the hosted Facet pages** (Unicode-safe) and adds an
  **Oberon-style System console** (`/system`, `/api/run`) fronting eval/type/SCArs.
- **Data visualization** in Latte (`lib/plot.lat`): bar/line/scatter charts to SVG via the
  CLI, the `/chart` GUI page, or `/api/plot`. The ML library gained a loss function (`mse`).
- **Starting Orpheus launches the GUI by default** (bare `latte`); the System console at
  `/` shows how to run every tool. A guide ships in `docs/visualization-and-ml.md`.
- **Interaction-combinator engine** (`src/icomb.rs`): Lafont's universal system
  (γ constructor · δ duplicator · ε eraser) with all six interaction rules and a confluent
  graph reducer — `latte icomb`, or `icomb` in the System console. It now also **compiles and
  runs programs**: an expression language (literals, `+`, `*`, `<`, and `if`, freely nested)
  compiles to interaction nets, reduces on the engine, and is **audited against the Loom
  interpreter**. Addition/multiplication lower to `Add`/`Mul`/`Succ`/`Zero` agents (multiplication
  duplicates a computed operand with the δ duplicator); comparison (`<`) reduces two Peano chains
  in lockstep to a Loobean; and `if` bundles its branches into a γ-cell that a selector projects
  with `Head`/`Tail`, erasing the unused branch with ε — so a computed Loobean drives real control
  flow, entirely by interaction. `(mul (add 2 3) 2)` reduces to 10 and
  `if (lt 2 3) then (add 10 5) else (mul 100 100)` to 15, both matching Loom; two 200-formula
  randomized batteries (nested `+`/`*`, and `+`/`*`/`<`/`if`) are cross-checked against the
  interpreter, alongside the annihilation, commutation, erasure-cascade, and confluence tests. `latte net "<expr>"` compiles a real Latte expression (its supported fragment) to a net and reduces it, printing the result and the interaction-step count next to the interpreter's answer. Beyond the bare `+`/`*`/`<`/`if` primitives the lowering now accepts a richer fragment — `let`, `+()`, `==`, **let-bound user functions**, and `loop … again(…)` — and the `if` is **lazy** (only the taken branch is built). **Unbounded recursion now runs on the net** as a genuine fixpoint: a self-recursive `let f = fn [n] -> … (f …) … in (f K)` compiles to HVM-style **`Ref` nodes** that the reducer unrolls lazily and collects against an eraser at the base case (factorial, sum, Fibonacci, and a 100-deep triangular all match the interpreter). An **active-pair worklist** in the reducer replaced the per-step rescan, taking recursion-heavy reductions from seconds to milliseconds (`fac 8`: 18.8 s → 17 ms) with confluence preserved (`docs/interaction-nets.md`).
- **The GUI opens in a window from GNOME.** Started in a desktop session, `latte`/`latte gui`
  pops a browser window (app-mode if available, else `xdg-open`); a `orpheus.desktop` entry and
  icon ship for the app grid. `--no-open` disables it; headless use is unaffected.
- **Adding your own library** is documented in `docs/adding-libraries.md` (with the worked
  `lib/vec.lat` example); data viz and ML modelling in `docs/visualization-and-ml.md`; building and running on Ubuntu and Windows in `docs/building-and-running.md`.
- **Language references, readable in the GUI.** The languages are documented in full:
  `docs/latte-language.md` (Latte — values, syntax, modules, semantics), `docs/facet-language.md`
  (Facet — the markup language with tool-call holes), `docs/scars-sound-changes.md` (the SCArs
  sound-change rule language, including stress- and cluster-conditioned changes), and
  `docs/interaction-nets.md` (the interaction-net engine and its compiled fragment). All of the
  manuals are also served **inside the running GUI at `/docs`** (sidebar + rendered Markdown) and
  are **editable in place** there (an Edit/Save toolbar writes the Markdown back to disk, Oberon-
  style), reachable from the System page's `Docs` link or the `System.Docs` command; the complete
  **Ligurian reference grammar is hosted at `/grammar`**.
- **Adaptive JIT compilation is the default.** Formulas are *interpreted while cold* and
  compiled to closures only once they get *hot* (re-entered past a threshold), so compilation is
  the default exactly when it pays for itself — including compile time. A one-shot like
  `(add 2 3)` is never compiled (≈ interpreter speed); a hot loop (e.g. 3000 gradient-descent
  steps) compiles after warm-up and beats the interpreter. The tree-walking interpreter remains
  the reference semantics; the compiler is checked against it by the whole suite and by
  `latte jit "<expr>"`, which compares interpreter / adaptive / forced-compile and times them.
- **Libraries can be added at run time** — no recompile. `import` consults a runtime registry
  first, so a `.lat` library can be loaded from a file (`latte eval --lib NAME=FILE …`), pushed
  over the network by a connected machine (`POST /api/lib`), or registered programmatically; it
  can itself import other libraries. **Every library is loaded by default** (`latte::all_libs`),
  so `eval`, the REPL, and the GUI console have the whole ecosystem in scope with no `import`.
  Library gathering is cached, so the standard libraries are parsed/compiled once and reused.
- **Compile a module from the GUI, Oberon-style.** The Compiler page (`/compile`, backed by
  `POST /api/compile`) takes a `core NAME …` module, compiles it (reporting any errors with
  line/column), and loads it into the running system; you can then `import` it or call it from
  the console immediately. `compile_and_register` does the same from Rust.
- **An optimizing Latte → Rust compiler ("Anvil", `latte rustc`).** Where the JIT compiles Loom
  formulas to closures at run time, Anvil is an *ahead-of-time* compiler that emits standalone
  Rust source for a Latte expression and its whole library closure. The emitted program carries a
  tiny self-contained noun runtime, so it builds with a stock `rustc` and runs natively with no
  dependency on this crate. It applies constant folding, lowers the arithmetic jets to native
  `u128` ops, eliminates unreachable arms, turns `let` into Rust `let`, compiles `loop … again`
  into a real Rust loop, and compiles lambdas to native closures (so `map`/`filter`/`foldl` and
  the entire chess engine compile straight through). `latte rustc "<expr>"` prints the Rust;
  `--run` builds and runs it; `-o file.rs` saves it. **Anvil is the default execution engine
  across the system:** `latte eval`, the `latte cli` prompt, and the **GUI / server console**
  (`/api/run`) all compile each expression to native code, cache the built binary in a persistent
  per-user directory keyed by a sha3 of the emitted source (so a program compiled once is reused
  across runs and reboots — recompilation happens only when the code actually changes; a warm run
  matches the interpreter's speed), run it,
  and render the resulting noun in that surface's own style — the compiled program emits a
  canonical form that each host parses back and prints (so the CLI keeps its `%cord` rendering and
  the GUI keeps its numeric rendering, both from the same computation). Builds are serialized and
  cached, so concurrent console requests compile once. `--interp` forces the tree-walking VM, and
  every surface falls back to it automatically whenever compilation isn't possible (`rustc`
  unavailable, an unsupported construct, or a runtime domain error), so a result is never silently
  wrong. (`latte repl` is the separate self-hosting environment for defining and introspecting
  arms, and stays on the interpreter.) The cache lives under `~/.cache/orpheus/anvil`
  (`%LOCALAPPDATA%\orpheus\anvil` on Windows; override with `ORPHEUS_CACHE`); `latte cache path`
  and `latte cache clear` manage it, and `latte eval --rebuild` forces a fresh compile. Verified
  bug-free by differential testing — the
  compiled-and-run output matches the interpreter exactly across
  arithmetic, lists, capturing closures, recursion, signed-number vectors, the full chess engine,
  and the learned ML evaluator, plus a randomized fuzzer of ~100 arbitrary nested formulas
  (arithmetic, comparisons, conditionals, lists, multi-capture closures, `let`/`foldl`/`map`),
  all agreeing with the interpreter on both values and failure modes. (Atoms are `u128` with checked arithmetic — but so is the
  interpreter, whose arithmetic jets reject larger atoms and crash on overflow/underflow/zero
  divisors; Anvil reproduces each failure mode on exactly the same inputs, verified by a
  boundary test covering `u128::MAX`, overflow, underflow, and divide-by-zero. The only
  theoretical edge — a tag literal over 16 bytes — never arises; the longest tag in the system
  is 5 bytes.)
- **A terminal command-line mode** (`latte cli`, also `latte console`). Starts an interactive
  Orpheus prompt with every library in scope: type a Latte expression to evaluate it, `:type EXPR`
  to infer a type, `:rust EXPR` to compile-and-run it natively through Anvil, `:libs` to list the
  loaded libraries, `:help`, and `:q` to quit.
- **A boardgame tool with machines as players** (`latte game chess`): each player is its own
  compiled Orpheus core (a "machine"), queried move by move by a game-agnostic match driver. The
  rules and AI are written entirely in Latte (`lib/chess.lat`, `lib/chessml.lat`) and run on the
  adaptive VM — legal move generation, king-safety, check/checkmate/stalemate, learned piece
  values, and an **alpha-beta minimax search with a positional evaluation** (the GUI's "play the
  model" searches two plies; it finds forced mates and stops hanging pieces). Two machines play a
  full game to a real checkmate. (Castling and en passant are omitted; documented in the file.)
- **A chess evaluator learned in Latte** (`lib/chessml.lat`): a linear model whose piece-value
  weights are fit by batch gradient descent in Latte against labelled positions. After a few
  hundred iterations it recovers sensible values — about `[1, 3, 3, 5, 9]` for P/N/B/R/Q — and
  drives the machine in **human-vs-machine** play: `latte game chess --human white` lets you type
  moves (`e2e4`) against the learned model.
- **Self-hosting the module system at scale.** The arm battery is laid out as a *balanced* tree,
  so a module's deepest arm has a small axis; modules of any size compile correctly (a previous
  right-nested layout corrupted addressing past ~64 arms) and arm lookup is shallower/faster.
- **The planner has a GUI** (`/plan`, `/api/plan`): enter a final demand and iteration depth and
  get the labour values and gross outputs computed in `lib/plan.lat` on the (now JIT-compiled) VM.
- - DONE since this revision: NATIVE NUMBER agents on the net (the HVM2 idea — a 64-bit
  literal is one agent and each arithmetic op ONE interaction: `gcd 1071 462` fell from
  72,758 to 116 interactions, `99999*99999` is 2; `--peano` keeps the unary mode) and a
  batch-claimed PARALLEL reducer (`latte net --par N`) that exercises uniform confluence
  and is test-verified equivalent to the sequential engine — a correctness demonstration,
  with HVM2-class lock-free throughput stated plainly as the remaining frontier. Also: a
  RAY TRACER written in Latte (`lib/trace.lat` — spheres, shadows, speculars, a reflection
  bounce; `latte trace` compiles it natively through Anvil, ~217 ms warm at 64x48), audited
  VECTOR JETS for the signed fixed-point kernel (nadd/nmul/ndiv/nlt/ndot/nsqrt — the test
  suite got 4.5x faster as a side effect), reverse-mode AUTODIFF over the nn layer algebra,
  S4D/Mamba-style %ssm + prenorm rmsnorm blocks, a TRAINED text classifier that reads
  context and negation and scores whole DOCUMENTS sentence-by-sentence, and HAR-RV
  volatility forecasting + embargoed validation + transaction costs + conformal bands in
  the trading stack.
- - DONE since the last revision: the interaction-net engine is complete for the numeric
  fragment — net-level `sub` and `==` agents, γ-pairs as net data (so user functions of ANY
  arity and multi-binding `loop`s compile to genuine net-level recursive definitions), a
  fully dynamic LAZY `if` whose branches are interaction-net boxes (Ref closures: only the
  taken branch is ever built, even when the condition is computed by the net itself), and
  dynamic `div`/`mod` as generated recursive nets. Run it with `latte net "<expr>"`,
  `latte eval --net`, or the `net` verb in the System GUI — every result is audited against
  the interpreter.
- - Not yet: bit-for-bit self-hosting of the whole compiler in Latte; and a *native desktop*
  tiling GUI (the browser GUI in a window stands in). The most intricate stress/cluster sound
  changes are now expressible — see `lib/breaking.sca` and `latte sca --file`.

## Files

| File | Role |
|------|------|
| `src/loom.rs` `src/knot.rs` `src/atom.rs` `src/sha3.rs` | Loom VM, Knot datatype, bignums, hashing |
| `src/latte.rs` | Latte compiler: expressions, modules, closures, **linker/imports** |
| `lib/std.lat`    | the standard library, written in Latte |
| `lib/mold.lat` `src/mold.rs` | the mold/aura type system + aura-aware rendering |
| `src/check.rs`   | the static type checker (`latte typecheck`) |
| `src/mocha.rs` `lib/mocha.lat` `lib/todo.lat` `lib/lexicon.lat` `lib/forge.lat` | Mocha environment + apps (Latte); Forge = team coding |
| `src/repl.rs`    | the self-hosting Latte environment (`latte repl`) |
| `lib/editor.lat` `lib/site/editor.html` | the WYSIWYG Facet editor: Latte document app + web page (Facet-file editing, Unicode) |
| `lib/tool.lat` | the system **command set**, written in Latte (`Tool.fib`, `Tool.primes`, …) — editable/recompilable from the GUI |
| `lib/site/system.html` | the Oberon-style System GUI: tiled text frames in tracks, middle-click-to-execute, marked viewers, Log, live module list, edit→compile→run |
| `lib/chessgame.lat` `lib/site/chess.html` | networked chess as a Mocha app (moves gossip between machines) + the interactive board page |
| `src/numerics.rs` `lib/num.lat` `lib/tensor.lat` `lib/ml.lat` | signed numbers, n-d tensors, ML training (Latte) |
| `src/viz.rs` `lib/plot.lat` `lib/site/chart.html` | data visualization: Latte layout → SVG, + chart GUI |
| `src/loom.rs` | the VM: 12-rule interpreter **and** the JIT (compile-to-closures, the default) |
| `src/icomb.rs` `lib/vec.lat` | interaction combinators + arithmetic net compiler; worked vector library |
| `src/plan.rs` `lib/plan.lat` `lib/site/plan.html` | economic planner (TANS) + its GUI |
| `src/game.rs` `lib/chess.lat` | boardgame tool (machines as players) + chess rules & AI in Latte |
| `src/rustgen.rs` | Anvil: the optimizing Latte → Rust compiler (`latte rustc`) |
| `lib/chessml.lat` | chess evaluator with piece values learned by gradient descent in Latte; human-vs-machine play |
| `docs/adding-libraries.md` | guide to writing and registering a new Latte library |
| `docs/latte-language.md` | the Latte language reference (values, syntax, modules, semantics) |
| `docs/facet-language.md` | the Facet markup-language reference (holes, directives, tool calls) |
| `docs/scars-sound-changes.md` | the SCArs sound-change rule-language reference |
| `docs/interaction-nets.md` | the interaction-net engine + its compiled Latte fragment |
| `lib/site/docs.html` | the in-GUI documentation viewer (sidebar + Markdown pane, served at `/docs`) |
| `lib/breaking.sca` | a worked stress- and cluster-conditioned ruleset (`latte sca --file`) |
| `dist/orpheus.desktop` `dist/orpheus.svg` | GNOME app-grid launcher + icon (opens the GUI in a window) |
| `docs/visualization-and-ml.md` | guide to charting and ML model design in Latte |
| `src/plan.rs` `lib/plan.lat` | planning calculations, *Towards a New Socialism* |
| `src/jets.rs`    | native arithmetic jets for the standard library |
| `src/agent.rs`   | counter + key-value agents (in Latte); the `add` jet |
| `src/sca.rs`     | SCArs host: rule parsing, categories/correspondence, graphemes, prosody |
| `lib/sca.lat`    | SCArs's multi-segment matching engine, written in Latte |
| `lib/ligurian.sca`| Solar→Heart change file for the Ligurian conlang |
| `src/facet.rs`   | Facet markup: parser, evaluator, tool dispatch (`SCArs`, `Txt`) |
| `src/serve.rs`   | Hymn HTTP/1.1 server: keep-alive, ETag/304, Range/206, fonts |
| `lib/site/index.facet` `lib/site/fonts/ligos-*.woff2` | the Ligurian page + bundled font |
| `src/net.rs` `src/store.rs` | distributed runtime + persistence + log compaction/GC |
| `src/main.rs`    | CLI: cli / node / eval / agent / selftest / bench / sca / evolve / serve / gui / mold / typecheck / mocha / plan / team / repl / tensor / ml / chart / icomb / jit / game / rustc / cache / net |
| `.github/workflows/release.yml` `scripts/build-all.sh` `DISTRIBUTION.md` `dist/` | binary distribution |
| `*_demo.sh`      | runnable demos |

A **trading advisor** (`latte trade`) calls the best model and recommends whether and how much to
trade. Following the honest finding that the dependable edge is volatility (not direction), it gives
a momentum-based directional lean with its *measured* out-of-sample hit rate, then sizes the position
by **fractional Kelly** (`f = 2W-1`, scaled to a quarter) combined with **volatility targeting**
(shrinking exposure when high volatility is predicted, capped at 2x). If the edge is not positive it
advises FLAT — stand aside. Flags: `--account N --kelly F --sentiment S`. Not financial advice.

**News sentiment** (`latte sentiment "<text>"`, `lib/sentiment.lat`) scores finance text with the
Loughran-McDonald method (polarity = (pos-neg)/(pos+neg), computed in Latte), an exogenous feature
most useful on equities/indices; it can be fed to the advisor with `--sentiment`.

The **GPU backend is auto-detected** (`gpu::detect_backend()` probes the NVIDIA driver, `/dev/nvidia0`,
and `nvidia-smi`): a present GPU accelerates ML and the GUI, an absent one falls back to the CPU
backend, so the system never relies on a GPU that is not there.
