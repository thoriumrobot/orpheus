# Orpheus update — the system teaches, measures, and unifies itself

This release is about closing distances: between defining a function and using it, between
guessing which engine is faster and knowing, between a tool living in one desk and serving
every desk, and between reading about an algorithm and watching it run.

## `def` — one line in any text becomes a function

Defining a session function used to require a detour:

```
eval (loop with [sq = (fn [x] -> (mul x x))] : (sq 3) end)
```

Now, in the System console, the CLI, or any text (the Oberon habit — the text you are reading
is also the program):

```
def sq [x] = (mul x x)                # shorthand
def cube = fn [x] -> (mul x (sq x))   # gate form; may call earlier defs
eval (cube 7)                         # → 343, from anywhere
def                                   # list them · undef NAME removes one
```

Definitions accumulate in a synthetic `user` module compiled through the same validating path
as `/api/compile` — a broken definition is rejected and the previous good set stays installed.
Because `user` joins the standard scope, a defined function is immediately callable from
`eval`, Facet pages, the CLI, and the HTTP API. One definition path, visible everywhere.

## The profiler — measured engine selection

The adaptive policy used to *guess* from the AST whether a program was worth compiling. Now a
**measurement replaces the guess** wherever one exists: every interpreter run above ~0.2 ms is
timed and recorded (in `profile.tsv` beside the compiled-program cache, keyed by program text
and library scope, smoothed across runs). A program whose measured time crosses the threshold
(default 1.5 ms; `ORPHEUS_PROFILE_NS` tunes it) is **compiled automatically before its next
run** — through the resident anvil daemon when one is up, so no call ever stalls on `rustc`.

`latte profile "<expr>"` runs both engines, persists the timings, and states the decision the
adaptive engine will now take. The interpreter measurement subtracts a scope baseline, so what
is priced is the expression, not the cost of linking the standard library around it.

## /learn — interactive tutorials, and algorithms you can scrub

A new hosted page, **`/learn`**, teaches the system with live widgets: the Latte language in
four editable boxes, one taste of each tool, and — the headline — **algorithms played under a
slider**. The new `lib/algoviz.lat` instruments bubble sort, insertion sort, binary search, and
a breadth-first maze flood: every comparison, swap, probe, and visit appends a frame to a
trace, a trace is ordinary data, and a frame renders through the same `gfx` scene pipeline
`latte gfx` uses. Facet tools `AlgoViz.frame(algo, xs, k, t)` and `AlgoViz.steps(algo, xs, t)`
drive it; drag `k` and the algorithm genuinely runs, on Loom, from the axioms up. The home
page and `/tools` link to it.

## One sentiment engine, every market

The Loughran-McDonald scorer advised the crypto/equity desk; the bond desk got none. Now the
engine has a **rates axis**: `bond_polarity` reads a hawkish/dovish policy lexicon and fuses
the general financial score *negated* — risk-off news is a Treasury bid, and "stocks rally on
strong growth" is bearish for bond prices. The same `news/` document stream feeds both desks
through one generalized scorer; `latte sentiment` prints both readings, `/api/sentiment`
returns both, `Sent.bond` serves Facet pages, and `latte trade --market bonds` fuses the model
(60%) with bond-scored news (40%), honoring `--news` and `--sentiment` like every other market.
The GUI's `/api/trade` now routes bond aliases to the bond desk instead of failing.

## A more settled bond model

`lib/finbond.lat` gains the two return-forecasting factors the literature settled on: the
**Cochrane–Piazzesi tent** over the curve's implied forwards (2·f₂₅ − y₂ − f₅₁₀ — information
level/slope/curvature do not span) and the **Cieslak–Povala cycle** (the 10y yield minus a slow
trend-inflation proxy — the stationary deviation is what forecasts returns, computed once as an
O(n) exponential average). The DB-backed training path projects all ten factors; the ablation
now drops *only* the money block against the full modern factor set, so it isolates the
stimulus signal honestly. Sizing is volatility-targeted fractional Kelly against the model's
own return series (`bvol`) — the discipline the crypto advisor gets from HAR-RV.

## Faster under the hood

- **Hymn's worker pool.** The server used to spawn a thread per connection with a hard `503`
  cliff at capacity. Connections are now handed to a fixed pool of reusable workers through a
  bounded backlog — bursts queue briefly instead of being turned away; only sustained overload
  sees a `503`.
- **The database read cache.** Rendered results of pure reads (get / query / history / agg /
  select / dash) are cached under a per-database generation stamp; any write bumps the
  generation, so a stale rendering is simply never hit again. Repeated GUI polls of an
  unchanged database now cost one Loom evaluation total.
- **γ-pairs on the interaction net.** `latte net` can now build pairs and take `head`/`tail`
  as Fst/Snd projection agents — reducer capability that existed but was never reachable:
  `latte net "head (tail [1 2 3])"` → 2, cross-checked against the interpreter.

## Housekeeping, honestly

The build is now **warning-clean**. Every dead item was either wired or removed: the SVG
renderer emitted root-less fragments (browsers refused them) — fixed and regression-tested;
superseded btc-only wrappers folded into their `_market` generalizations; the assembled
sound-change files' `:: changes:` header is now surfaced by `latte sca --file`; HEAD handling
uses its accessor; a stale claim that `(dec 0) = 0` in the standard library's comments was
corrected (it crashes, as it should). The sparsest data-intensive libraries (the B-tree's
split/borrow/merge, the LSM merge, consistent hashing's ring walk, the CRDT join) gained
line-level commentary to match the rest of the shelf.
