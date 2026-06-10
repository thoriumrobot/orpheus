# Interaction Nets — the Parallel Backend

Orpheus includes an **interaction-net** engine: Lafont's interaction combinators (the **γ**
constructor, **δ** duplicator, and **ε** eraser) as a confluent graph reducer, plus a compiler
from a fragment of [Latte](latte-language.md) into nets that *compute by local rewriting*.
Lafont's strong confluence means the parallel reduction order does not change the result, so a
net's normal form is canonical — and the engine is kept in agreement with the reference Loom
interpreter by differential testing on every run.

This is the spec's "optional interaction-combinator accelerator" (§3.4, §10.2), realised for an
arithmetic-and-control fragment.

---

## 1. The reducer

Three symbols, six interaction rules (annihilation when two like agents meet on their principal
ports, commutation when unlike agents meet, erasure when ε meets anything). On top of the bare
combinators the engine carries arithmetic agents — `Zero`/`Succ` (Peano naturals), `Add`,
`Mul`, `Lt`, and a selector for `if` — each with its own interaction rules, so a compiled
expression reduces to a Peano chain that decodes back to a number.

`latte icomb` prints a narrated tour of the six rules and a confluence check.

---

## 2. The compiled fragment

`latte net "<expr>"` parses Latte, lowers the supported fragment to a net, reduces it, and
prints the value alongside the interaction-step count and the interpreter's answer for
cross-checking.

The **primitive** agents are naturals under `add`, `mul`, `lt`, and `if`. On top of those, the
lowering accepts a richer source fragment by **expanding it into the primitives at compile
time**:

| Construct | How it reaches the net |
|-----------|------------------------|
| `add` / `mul` / `lt` | native net agents |
| `+(e)` | `add e 1` |
| `==` | folded to a Loobean (`0`/`1`) |
| `sub` / `dec` / `div` / `mod` | constant-folded (no native agent) |
| `let x = e in …` | bind and substitute |
| `let f = fn [x] -> … in …` | a **user-defined function**, inlined at each call |
| `loop … again(…)` | **bounded unrolling** — the net's form of recursion |
| `if` | **lazy** (see below) |

Anything outside the fragment — lists, cells, cords, `case`, unbounded recursion — is reported
as unsupported rather than mis-evaluated.

```sh
latte net "(mul (add 2 3) 2)"                                   # 10
latte net "let sq = fn [x] -> (mul x x) in (add (sq 3) (sq 4))" # 25  (user function, inlined)
latte net "loop with [acc = 0, i = 5] : if (i == 0) then acc else again((add acc i), (dec i)) end"  # 15
```

### User-defined functions and recursion

A `let`-bound gate is a user-defined function; each call is inlined into the net, so
`let dbl = fn [x] -> (add x x) in (dbl (dbl 6))` builds a net of `add` agents and reduces to
`24`. Recursion is provided by `loop … again(…)`, which is **unrolled** at compile time up to a
fuel budget; because the conditions of a closed expression fold to constants, the loop
terminates and lowers to a finite net (e.g. a sum loop becomes a chain of `add` agents). This is
the net's bounded form of general recursion; unbounded recursion via net-level fixpoints remains
future work (§ Frontier).

### Lazy `if`

`if` is **lazy on the net**: the compiler evaluates the (closed) condition and lowers **only the
taken branch**, so the untaken branch is never built into the net and never reduced. For
`if (lt 7 4) then (mul 6 7) else (add 1 1)` the condition is false, so the net contains only
`add 1 1` — the `mul 6 7` is absent entirely (verifiable from the agent counts and step count).
When a condition is genuinely non-constant, the compiler falls back to the strict net selector,
which bundles both branches and projects one (erasing the other) once the condition resolves.

---

## 4. General recursion via REF nodes (net-level fixpoints)

Pure interaction combinators are affine — `λx.(x x)` is inexpressible — so unbounded recursion
needs an extension. The net uses the same device as HVM: a **`Ref` node** that names a top-level
definition (a closed net) and is *unrolled lazily by the reducer*. The rule is demand-driven:

- when a `Ref`'s principal meets a **consumer**, it is dereferenced — a fresh copy of the body is
  built into the net and wired in place of the reference;
- when it meets an **eraser**, it is simply collected.

That second case is what lets recursion **halt**: a function's `if` compiles so that each branch is
itself a `Ref` (a closure over the parameter); the selector wires the taken branch's `Ref` to the
result (so it unrolls) and erases the other (so its recursive call is dropped instead of expanding
forever). The argument is duplicated to its uses with δ, and `dec`/`sub`-by-a-literal use a small
`Pred` agent. A `let f = fn [n] -> … (f …) … in (f K)` whose body actually mentions `f` is detected
and run on this engine; everything else still takes the ordinary (folding) lowering.

Because the recursion lives in the reduction graph, depth is bounded only by the step budget, not by
the host stack. The engine is cross-checked against the Loom interpreter's `loop` formulation of the
same function (Latte's own `let` is non-recursive, so a `loop` is the interpreter-side reference):

```
latte net "let fac = fn [n] -> if (lt n 1) then 1 else (mul n (fac (dec n))) in (fac 8)"   → 40320
latte net "let sum = fn [n] -> if (lt n 1) then 0 else (add n (sum (dec n))) in (sum 10)"   → 55
latte net "let fib = fn [n] -> if (lt n 2) then n else (add (fib (sub n 1)) (fib (sub n 2))) in (fib 10)"  → 55
latte net "let tri = fn [n] -> if (lt n 1) then 0 else (add n (tri (dec n))) in (tri 100)"  → 5050
```

The supported surface was single-argument natural-number recursion; the **general compiler**
(§7a below) has since removed that limit.

## 5. Performance — the active-pair worklist

Finding the next redex used to scan every agent (and allocate a fresh index vector) on each step:
O(n²) over a reduction, which dominates once recursion makes the net large. The reducer now keeps an
**active-pair worklist** — wiring a principal port pushes the affected agents — and drains it instead
of rescanning, falling back to a single linear scan only to confirm the normal form (which also makes
the fast path safe: no redex can be missed). The win is large on the recursion-heavy nets above:

| program | before | after |
| ------- | ------ | ----- |
| `tri 150` | 4847 ms | 58 ms |
| `fac 8`   | 18760 ms | 17 ms |

Reduction order still does not affect the result; the confluence battery (forward worklist vs.
reverse scan) passes unchanged.

## 6. Correctness

For every compiled expression the engine's value is checked against the Loom interpreter. Two
200-formula randomized batteries (arithmetic, and arithmetic-with-`if`), the worked fragment cases,
and the recursion cross-checks keep the net and the interpreter in agreement.

## 7. The general compiler — pairs as net data

Both items that used to sit on the frontier — a fully dynamic lazy `if` and multi-argument
net-level recursion — are now implemented, and one idea delivers both: **γ-cells as data**.

**New agents.** `Sub`/`SubB` compute monus and `Eq`/`EqZ`/`EqB` compute equality by the same
Peano-lockstep walk `Lt` uses, so `(sub a b)` and `a == b` reduce as net interactions on
arbitrary (non-constant) operands. Both return the usual Loobean (0 = true).

**Pairs.** A γ-cell whose children are values is a pair; `Head`/`Tail` project out of it (the
rules already existed for the lazy-`if` bundle). The compiler packs a function's arguments into
a right-nested γ-chain, hands ONE packed value to the `Ref`, and each parameter occurrence
takes a δ-copy of the pack and projects its component — δ⋈γ commutation does the sharing.
That single device gives:

- **functions of any arity** — `let f = fn [a b] -> … in (f 4 2)` compiles to a definition
  over the packed pair;
- **multi-binding `loop`s** — a loop lowers to a recursive definition over its bindings, with
  `again(…)` a `Ref` call on the re-packed values: `loop with [a = 0, b = 1, i = 10] : …`
  runs as genuine net recursion, not unrolling;
- **dynamic `div` and `mod`** — lowered to generated recursive definitions built from the
  net's own `Sub`/`Lt` agents (`div` recurses over the packed `[x q b]`, `mod` over `[x b]`),
  so `(div (f 15) 5)` reduces on the net even though the dividend is unknown at compile time;
- **mutual flows like `gcd`** — `let gcd = fn [a b] -> if (b == 0) then a else (gcd b (mod a b))
  in (gcd 1071 462)` → 21, tens of thousands of interactions, every one a local rewrite.

**The lazy dynamic `if`.** Every `if` whose condition does not constant-fold compiles its
branches into **interaction-net boxes** — `Ref` closures over the packed argument. The
selector wires the taken branch's `Ref` to the result (it unrolls on demand) and points an
eraser at the other (it is collected *unexpanded*). The proof is in the test suite: an `if`
whose condition is itself computed by a recursive net call, with `(mul 99999 99999)` in the
untaken branch, reduces in a few hundred steps — the ~10¹⁰ interactions the strict selector
would have paid are simply never built.

**Using it.** `latte net "<expr>"` and `latte eval --net "<expr>"` run the general compiler
(falling back to the older single-parameter and simple-expression paths where those are
smaller), and the System GUI's `net <expr>` verb does the same with the interpreter audit
shown inline. Three compilers, one rule: when the net and the interpreter both produce a
value, they must agree — the randomized batteries and the new `general_compiler_matches_interpreter`
test hold that line.

## 8. Frontier

Still open: net-level cells/lists as *surface* data (pairs are internal machinery today), and
sharing `let`-bound subnets instead of rebuilding them per use (pure duplication is correct
but can repeat work).
