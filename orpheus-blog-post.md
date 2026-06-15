# Orpheus: a whole functional computing environment in one dependency-free binary

Most software today is an iceberg of dependencies. A "hello world" web service can pull in
hundreds of packages; a language runtime drags along a garbage collector, a JIT, a standard
library the size of a city. Orpheus is a deliberate experiment in the opposite direction: a
complete, self-contained functional computing environment — a virtual machine, a programming
language, a compiler, a web server, a GUI, a distributed runtime, and a pile of tools — written
in pure Rust with **zero external crates**. It builds offline, runs deterministically, and fits
in a single binary called `latte`.

This post walks through what's inside and why it's interesting.

## The shape of the system

Orpheus is built in layers, each a small idea that the next one stands on:

- **Knot** — the data. Every value is a *noun*: either an arbitrary-precision atom or a pair of
  nouns (a cell). That's the whole type system. Lists, records, text, code, and serialized state
  are all just nouns. Nouns serialize to bytes (`jam`) and back (`cue`), and every noun has a
  content hash, which is what makes the distributed layer work.
- **Loom** — the virtual machine. A tiny Nock-style combinator evaluator: twelve reduction rules
  over nouns (slot, constant, apply, increment, equality, if, compose, push, invoke, edit, …).
  It is the reference semantics for everything above it.
- **Latte** — the language. A small functional language with pattern-style `case`, tail-recursive
  `loop`/`again`, first-class lambdas (`fn [x] -> …`), and modules (`core`). Latte compiles down
  to Loom formulas. Truth is Loobean (0 = true), lists are 0-terminated cells, and calls are
  parenthesized: `(add (mul 6 7) 1)`.
- **The libraries** — written in Latte itself: a standard library (`std`), signed fixed-point and
  vector/tensor math (`num`, `tensor`, `vec`), a linear-regression learner (`ml`), an economic
  planner (`plan`), plotting (`plot`), a full chess engine (`chess`), and a chess evaluator whose
  piece values are *learned by gradient descent in Latte* (`chessml`). On top of those sit two
  large **teaching collections** — fifteen coding-interview technique modules and a
  data-intensive (DDIA) library set, each written to be read and paired with a live tutorial
  (described below).
- **The surfaces** — Facet (a markup language), Hymn (a web server), a browser GUI, SCArs (a
  sound-change engine for constructed languages), Mocha (an app environment), Forge (collaborative
  coding), and a content-addressed distributed runtime.

Everything is one program. The same `latte` binary is the VM, the compiler, the server, and the
CLI for all of the above.

## Four ways to run your code

The most unusual thing about Orpheus is that a Latte expression can be executed by four different
engines, and they all agree with each other — a property that is continuously checked by the test
suite rather than hoped for.

**1. The interpreter.** Loom walks the formula tree directly. This is the reference: simple,
obviously correct, and the yardstick everything else is measured against.

**2. The adaptive JIT.** Formulas are interpreted while *cold* and compiled to native closures only
once they get *hot* — re-entered past a threshold. So compilation happens exactly when it pays for
itself. A one-shot like `(add 2 3)` is never compiled; a 3000-iteration training loop is.

**3. Anvil — the optimizing Latte→Rust compiler.** This is the headline tool. Anvil takes a Latte
expression and its entire library closure and emits a **standalone Rust program** carrying a tiny
self-contained noun runtime — no dependency on Orpheus itself. It constant-folds, lowers the
arithmetic primitives to native `u128` operations, eliminates unreachable functions, turns
`let` into Rust `let`, compiles tail-recursive `loop` into a real Rust loop, and compiles lambdas
into native closures. The entire chess engine and the gradient-descent learner compile straight
through. You can see the generated Rust (`latte rustc "<expr>"`), save it (`-o file.rs`), or build
and run it (`--run`).

Anvil is also the **default execution engine**: `latte eval`, the interactive `cli`, and the GUI
console all compile your expression to native code, cache the built binary, and run it. The cache
is content-addressed — keyed by a hash of the generated source and stored in a persistent per-user
directory — so a program compiled once is reused across runs and reboots. Recompilation happens
only when the code actually changes. A warm run is as fast as the interpreter; a cold one pays for
`rustc` once. If `rustc` isn't installed, everything transparently falls back to the interpreter,
so the system always works.

How do we know Anvil is correct? Differential testing. For hundreds of expressions — arithmetic,
lists, capturing closures, recursion, signed-number vectors, the whole chess engine, the learned
evaluator, plus a randomized fuzzer of arbitrary nested formulas — the compiled-and-run output is
checked against the interpreter, on both the values produced *and* which inputs fail. They match,
including subtle things like overflow and divide-by-zero behaving identically on both sides.

**4. The interaction-net compiler.** This is the research corner. Orpheus implements Lafont's
interaction combinators (the γ constructor, δ duplicator, ε eraser) as a confluent graph reducer,
and compiles a fragment of Latte into interaction nets that *compute by local rewriting*.
Multiplication duplicates an operand with the δ duplicator; the unused parts of a computation are
erased with ε. The net's primitive agents are naturals under `+`, `*`, `<`, and `if`, but the
compiler now accepts a richer source fragment by expanding it into those primitives: `let`,
`+(…)`, `==`, **let-bound user functions** (inlined at each call site), and `loop … again(…)`.
The `if` is **lazy**: the compiler evaluates the closed condition and builds *only the taken
branch* into the net, so the unused branch is never constructed and never reduced. And recursion
now runs *on the net itself*: borrowing HVM's trick, a top-level definition becomes a **`Ref`
node** that the reducer unrolls lazily when something demands its value and **collects against an
eraser** when it does not — exactly what lets a recursive function stop instead of expanding
forever. So a self-recursive factorial reduces to 40320 by genuine net-level fixpoint, and `fib`,
`sum`, and a 100-level `tri` all check out. A small change — draining an *active-pair worklist*
instead of rescanning the whole net each step — took the heaviest of these from seconds to
milliseconds (`fac 8`: 18.8 s → 17 ms). As for that lazy `if`:
`latte net "if (lt 7 4) then (add 1 1) else (mul 6 7)"` reduces to 42 on the net engine with the
`add 1 1` absent entirely; `latte net "let sq = fn [x] -> (mul x x) in (add (sq 3) (sq 4))"`
reduces to 25. Both agree with the interpreter, which — along with two 200-formula randomized
batteries — is what keeps the net honest.

## A tour of the tools

Everything is a subcommand of `latte`:

- `latte eval "<expr>"` — evaluate an expression (compiled natively by default; `--interp` to
  force the interpreter, `--rebuild` to ignore the cache).
- `latte cli` — an interactive console: type expressions, `:type` to infer a type, `:rust` to
  compile-and-run natively, `:libs` to list libraries.
- `latte repl` — the *self-hosting* environment, where you define your own arms and introspect
  them.
- `latte rustc "<expr>" [--run] [-o f.rs]` — the Anvil compiler.
- `latte net "<expr>"` — the interaction-net compiler.
- `latte icomb` — a narrated tour of interaction-combinator reduction.
- `latte jit "<expr>"` — compare interpreter / adaptive / forced-compile and time them.
- `latte game chess [--human white]` — two machines play a full game to checkmate; the machine
  player is its own compiled Orpheus core, and with `--human` you can play against the learned
  evaluator (now an **alpha-beta search with a positional eval**, not just a one-ply grab) by
  typing moves like `e2e4`.
- `latte ml [linear|perceptron|kmeans|knn]`, `latte tensor`, `latte chart`, `latte plan` — train a
  model (linear regression, a perceptron, k-means, or k-NN — all in Latte), do n-dimensional
  tensor math, render an SVG chart, or solve a little planning problem, all computed in Latte
  libraries on the VM.
- `latte nn` — a neural network in Latte: composable layers (`%dense`/`%relu`/`%tanh`/`%res`)
  folded by `net_fwd`, with residual blocks for ResNet-style nets, and a one-hidden-layer MLP
  trained by **backpropagation**. The demo learns `y = |x|` (loss ≈ 2.7 → 0) and writes a loss
  curve. Deeper nets are just longer layer lists.
- `latte fin` — practical financial ML after Lopez de Prado: momentum + realized-volatility
  features, train-only standardization, a **walk-forward split**, and logistic regression. After a
  price-only model proved near-chance on gold, it was moved to where momentum and volatility
  clustering are strongest — **Bitcoin** (1300 daily BTC/USD closes, 2022–2026). Daily direction
  stays near-random, but the default **volatility-regime** task earns a genuine **+3–4 point
  out-of-sample edge** (≈ 55% vs ≈ 51% baseline); the same model gets +50 points on a synthetic
  mean-reverting series. Plots a variance-timing equity curve. A working, leak-aware pipeline — not
  a profit machine.
- `latte gfx` — a **graphics library**: a scene is a list of tagged shapes
  (`%line`/`%rect`/`%circle`/`%poly`/`%text`) over packed-RGB colours, built in Latte and rendered
  to SVG by the host. The structure of a drawing is ordinary data.
- `latte gpu` — a **data-parallel GPU compute library**: buffers and kernels (`map`/`zipWith`/
  `reduce`/`saxpy`/`dot`/`matmul`/`shade`) as data, targeting an **NVIDIA GeForce RTX 4070 Ti
  SUPER** (16 GB; multi-core-CPU reference backend in this zero-dependency build, CUDA being a
  drop-in swap). It integrates with `nn`/`ml` (matmul is the dense-layer kernel) and `gfx` (a
  parallel Mandelbrot shader renders through the graphics path).
- `latte sca <words>` — evolve words through ordered sound changes (for constructed languages);
  `latte sca --file rules.sca <words>` applies a whole rule file, including stress- and
  cluster-conditioned changes (see `lib/breaking.sca`).
- `latte team --as NAME --share CODE` — collaborative coding across machines, with attribution.
- `latte cache [path|clear]` — manage the compiled-program cache.
- `latte gui` — the web GUI: a System console, a WYSIWYG document editor, charts, the planner,
  an Oberon-style **module compiler page** where you paste a `core` module, compile it (with
  line/column errors), and have it loaded live into the running system; the **manuals
  themselves, served at `/docs`** and now **editable in place** (an Edit/Save toolbar writes the
  Markdown straight back to disk, true to Oberon's every-text-is-a-document spirit); and the
  complete **Ligurian reference grammar hosted at `/grammar`**.
- `latte <module> [<topic>]` — run a coding-interview **technique demo**: `latte dp
  optimisation` (0/1 knapsack, LCS), `latte greedy gas`, `latte search answer`, and a dozen
  more (`algo`, `dsa`, `wgraph`, `numth`, `bits`, `strings`, `grid`, `design`, `trees`,
  `intervals`, `graphs`, `backtrack`). Each prints a worked table; the same arms are callable
  directly (`latte eval "(se_isqrt 17)"`).
- `latte ddia [<topic>]` — run the **data-intensive** demos following Kleppmann's DDIA:
  `latte ddia bloom`, `lsm`, `mvcc`, `raft`, `crdt`, `hll`, … each a runnable example of the
  technique, with a composed database (`lib/db.lat`) tying them together.
- `latte node` — join a content-addressed, event-logged distributed runtime.

## A library you can study from

Beyond the demos, Orpheus carries a teaching collection: a few dozen Latte libraries that work
through the standard coding-interview and data-systems canon, each written to be *read* — every
line commented — and each paired with an interactive tutorial.

Fifteen **technique modules** cover the recurring interview categories: algorithm paradigms,
data-structure patterns, weighted graphs, number theory, bit manipulation, strings, grids,
data-structure design, binary trees, dynamic programming, intervals, binary search, graph
decision problems, backtracking, and greedy algorithms. A separate set follows Martin
Kleppmann's *Designing Data-Intensive Applications* — B-trees and LSM-trees, Bloom filters,
vector clocks, CRDTs, consistent hashing, MVCC, Raft consensus, HyperLogLog and Count-Min
sketches — and assembles them into one small but real composed database (`lib/db.lat`: an
LSM-tree store with a write-ahead log, a Bloom filter, a secondary index, and MVCC snapshot
isolation). Building these stretched the standard library too, which gained a bitwise toolkit, a
general merge sort, and cord↔byte conversion along the way.

What makes them more than a code dump is the way you read them. Each technique has a CLI demo
(`latte dp optimisation`, `latte greedy gas`, `latte ddia bloom`) and, in the GUI, an
**interactive tutorial**: a live text whose every framed example runs against the real library
as the page loads, so you read the explanation and watch the actual result appear together. Edit
a line, middle-click, and it re-runs. The tutorials are written at a semantic level — they
explain *why* each algorithm is correct (the loop invariant, the exchange argument, the
recurrence), not just what it does. For newcomers, a **"Start here" primer** front-loads how to
read the code and the handful of conventions everything uses — including the live-by-hand trace
of a binary search — and the data-intensive guide opens with a plain-language on-ramp explaining
the field's vocabulary; every tutorial links back to them. The whole collection is covered by the
differential test suite, so a regression in any arm fails the build.

## Some engineering worth calling out

**Zero dependencies, fully offline.** There is no `Cargo.lock` full of transitive packages. The
SHA-3 hashing, the arbitrary-precision arithmetic, the HTTP server, the JSON-free wire format —
all hand-written. This makes the whole thing auditable and reproducible: the source you read is
the software you run.

**Determinism and content-addressing.** Nouns serialize canonically and hash with SHA-3, so the
same computation always produces the same bytes and the same hash. The distributed runtime is an
event log of content-addressed actions; peers converge by replaying the same events.

**Oberon-style live recompilation.** Libraries are Latte source, and you can edit and reload them
*into a running system* — from the command line (`--lib NAME=FILE`), over the network, or from the
GUI's compiler page — without rebuilding the binary. The whole system can then `import` the new
module immediately.

**A real bug, found and fixed.** Modules used to lay their function table out as a right-nested
tree, which made the deepest function's address grow exponentially and silently corrupt addressing
once a module passed ~64 functions. The fix was a *balanced* layout, so modules of any size compile
correctly — a nice example of the kind of subtle issue that only shows up when you self-host a
language's module system at scale.

**Correctness by cross-checking.** Because there are multiple engines and a reference interpreter,
"is it right?" has a concrete answer: run it both ways and compare. That discipline — differential
testing against a simple reference — is what lets a one-person system layer a JIT, an optimizing
native compiler, and an interaction-net reducer on top of the same language and trust all of them.

## Trying it

Orpheus builds on Ubuntu and Windows with nothing but a Rust toolchain (1.75+):

```sh
cargo build --offline --release
./target/release/latte eval "(mul (add 3 4) 5)"   # → 35
./target/release/latte game chess
./target/release/latte gui                          # http://127.0.0.1:8088/
```

The distribution ships the prebuilt binary alongside the complete source, so you can run it
immediately or rebuild it in place. Full platform instructions — including the Windows toolchain,
the runtime `rustc` requirement for native execution, and the compiled-program cache — are in the
building-and-running guide.

## What's still open

Orpheus is honest about its frontier. The interaction-net compiler now handles a first-order
fragment with `let`, user-defined functions, a lazy `if` that builds only the taken branch, and
**genuine net-level recursion** (single-argument fixpoints unrolled by the reducer) — what's left
there is *multi-argument* net recursion and a fully *dynamic* lazy `if` for non-constant
conditions, which awaits interaction-net boxes. (The affine-variable limit of pure interaction
combinators, `λx.(x x)`, is still why Loom and not the net is the canonical core.) There's no
native desktop GUI yet — the browser GUI stands in — and bit-for-bit self-hosting of the whole
compiler in Latte remains future work. (Unbounded net recursion, and stress- and
cluster-conditioned sound changes, once listed here, are now implemented.) None of that detracts
from the core idea, which the system already demonstrates end to end: that a complete,
multi-engine, self-hosting functional environment can be small, dependency-free, deterministic,
and verifiable — and still do real work, from playing chess to learning piece values to compiling
itself to native code.

- `latte trade` — an **automatic trading advisor**: it calls the best model and recommends whether
  and how much to trade, sizing positions with **fractional Kelly + volatility targeting** and
  standing aside when the edge is not positive. `latte sentiment` adds Loughran-McDonald **news
  sentiment** as an optional input. (Research demo, not financial advice.) The GPU backend is
  **auto-detected** — used when present, with a transparent CPU fallback when not.
