# Orpheus update — the system got fast, and it teaches the GUI

Two threads this release: measured performance work on the evaluation pipeline, and a
hands-on tutorial for working in Latte from the GUI.

## The scope-core cache: evaluation without the linking tax

Profiling showed every evaluation re-merging and re-compiling the full ~60-library scope
— ~33 ms of linking before a single Loom step ran. That tax was paid by `eval` in the
System console, by every Live widget on a Facet page, by the adaptive engine's
interpreter path, by the CLI. The remedy is the classic one for Nock-family systems
(compile once, reuse — the shape of Vere's bytecode and memo caches): the scope is now
compiled **once** per (library set, generation) into a core with a placeholder `__main`
arm, and each evaluation compiles only its own expression and splices the formula into
the cached core at `__main`'s leaf — an O(log n) rebuild over shared structure.

Measured on the release build:

- per-evaluation linking overhead: **~33 ms → ~0.01 ms**
- a fresh `Latte.eval` widget expression: **< 1 ms** — the box now follows your typing
- a warm one-shot `latte eval` (native): **~4 ms end to end** (persisted key memo)
- `/learn` warm render: **3.1 s → ~1 ms** (the whole-page memo); virgin-cache cold
  render **36 s → ~2 s** (detached warming instead of synchronous rustc stalls)

A `def`, a runtime module compile, or a library edit bumps the generation; the next
evaluation rebuilds the scope once and everything after it flies again.

## The warm native path: no codegen to find a binary

Finding a compiled program in the Anvil cache requires its content hash — which was
computed by regenerating the program's full Rust source on every warm run (twice, in
fact: once to look, once to run). The (expression, scope, generation) → key mapping is
now memoized in-process **and persisted** (`nkeys.tsv` beside the cache) for
generation-0 scopes, keyed by a fingerprint of the running executable — so editing any
shipped library and rebuilding invalidates every line by construction, while a warm
one-shot `latte eval` becomes a stat, a lookup, and a spawn.

## Pages memoized whole; builds never block a request

A Facet render is a pure function of (source, parameters, tool registry) — the module has
said so in its header all along — so rendered pages are now memoized outright, keyed by
library generation and day. And when the adaptive engine wants a program compiled with no
daemon up, it spawns a detached warm child instead of building in the serving thread
(recursion-guarded, and disabled under `cargo test`, where re-executing "yourself" means
re-running the test harness — a lesson the load average taught emphatically). The
`Latte.eval` page tool also graduated from a std-only scope to the FULL scope — a `def`
made in the System console is instantly callable from any page widget — which the
scope-core cache made affordable.

## Using Latte from the GUI — the tutorial

A new document on the shelf, `docs/using-latte-from-the-gui.md`, walks the whole loop:
first evaluations and the three ideas that dissolve most confusion (0 is true, lists end
in 0, calls are parenthesized); running command lines from ANY text; `def` for session
functions; `System.New` → **Compile** → the running image → **Store** → a permanent
package; tools that render (`[%html …]`/`[%svg …]`, gfx scenes); Latte on live pages
(`Latte.eval` boxes, `/learn`, `/tools`); and how the engines choose. The `/learn` page
gained a matching panel that links it, with a live evaluator sharing the console's
scope — `def` something in the System viewer and call it from the page.

One correction landed alongside: the standalone Latte tutorial had claimed a
`let`-bound gate may call itself. On `eval` it may not (the binding is not in scope in
its own body — recursion is `loop`/`again`, module arms, or `def`); the claim had been
"verified" against a library arm that happened to share the example's name. The
interaction-net engine is the genuine exception: `latte net` compiles self-recursive
`let` to lazily-unrolled Ref nodes.

## Documentation

`docs/building-and-running.md` documents both caches next to the profiler section;
the README indexes the new tutorial; and the `/learn` page carries the GUI workflow
panel. As always, nothing here changed the philosophy: same axioms, same engines,
same answers — measured, and now without the ceremony.
