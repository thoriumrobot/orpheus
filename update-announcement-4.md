# Orpheus update — first-class recursion, three engines abreast, a bond desk that explains itself

## Gates recurse — everywhere

A `let`-bound gate can now call itself: the binding is in scope inside its own body,
and the gate stays an ordinary value while recursing (pass it to `map` mid-descent).

```
let f = fn [n] -> if (lt n 2) then 1 else (mul n (f (dec n))) in (f 5)     → 120
let gcd = fn [a b] -> if (b == 0) then a else (gcd b (mod a b)) in (gcd 1071 462)
```

The three engines implement it three ways and agree: the interpreter generalizes the
loop/again trap idiom (the gate's own core answers to its name at axis 1), the native
backend ties a reference knot, and the interaction net unrolls a `Ref`. A like-named
parameter shadows the self-reference; mutual recursion between two `let`s remains
inexpressible by design — module arms own that. Pinned by a differential test battery.

## Higher-order Latte reads like it should

Two companion changes make user-defined higher-order functions natural. A **computed
gate is callable** — `((compose f g) x)`, `((fn [x] -> (mul x x)) 7)`, `((nth gates 1) 5)`
— via a parser desugaring to a reserved `__call` binding. And **arms eta-expand to
gates**: name an arm where a gate is expected and it becomes one, so `(foldl add 0 xs)`,
`(map dec xs)`, `(sortby lte xs)`, and `((flip sub) 3 10)` all just work. The standard
library gained the combinators `compose`, `flip`, and `applyn`.

## The interaction net keeps pace

A pre-normalization pass runs before the net compiler: `case` lowers to comparison
chains (tags are atoms; short atoms are native net numbers), and calls to non-recursive
higher-order gates are β-reduced at compile time with capture-avoiding substitution —
so currying, `compose`, and `flip` now run on the net, cross-checked per invocation
against the interpreter. Recursion still goes through `Ref` unrolling, untouched.

## One rustc at a time

Native builds now serialize across PROCESSES on a lock file beside the cache (stale
locks broken after ten minutes; the guard cleans up even on panic). A widget-heavy
page warming a virgin cache queues its compiles instead of racing them.

## The bond desk explains itself

The shared logistic core gained ridge (L2) training — `lr_train_l2`, bias unpenalized,
the standard Hoerl-Kennard cure for correlated factor sets. A λ sweep at 4,000
iterations measured the teaching series insensitive to mild shrinkage (69.6% out-of-
sample at λ ∈ {0, .02, .05}; over-shrinking costs a point), so λ=0.02 is kept for
coefficient stability at zero accuracy cost — the pinned canon is unchanged. The
advisor now decomposes the latest month's signal into named per-factor drivers:

```
top drivers now   : curve level +40%  ·  Cieslak-Povala cycle +39%  ·  curve slope +21%
```

## And the rest

AlgoViz gained instrumented **quicksort** (Lomuto, a frame per pivot comparison, the
range worklist explicit), on `/learn` and `/tools` beside the other four. The
data-intensive libraries took another line-level commentary pass — Bloom's
Kirsch-Mitzenmacher probes, Merkle's odd-carry pairing, vector-clock comparison,
Raft's append/commit walks, varint continuation bits, Lamport's tie-break. A failed
detached-warm spawn now retries. The tutorial, the language reference, and the GUI
guide all teach the new recursion story; the reference documents `__call` and
eta-expansion precisely.
