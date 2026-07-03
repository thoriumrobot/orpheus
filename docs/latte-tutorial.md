# The Latte Tutorial

Latte is the functional language of the Orpheus environment. It is small on purpose:
every program compiles down to **Loom**, a 12-rule virtual machine whose only datatype
is the **Knot** — an *atom* or a *cell*. The standard library, the type system, and
Orpheus's own tools are all written in Latte, on top of a single arithmetic primitive
(successor). This tutorial builds the language up from that primitive.

## How to learn from inside the GUI

This document is not just readable — **it runs**. Open the system and this text:

1. Start the GUI (`latte gui` from a shell) and open the printed address.
2. Click **Docs** in the toolbar, then **The Latte Tutorial** — this document opens
   as a text in a viewer.
3. **Middle-click any command line below** (or put the caret on it and press
   Ctrl/⌘+Enter). The line runs against the live system and the result appears in
   the **Log** viewer. Runnable lines are tinted like this one:

eval (add 2 3)

4. The text is yours: edit an example in place and middle-click it again. Lines
   beginning with `::` are comments — the runner skips them, so the expected
   results printed below each example never execute by accident.

Everything that follows works the same from the System console's command line, from
`latte cli` in a shell (`latte eval "<expr>"` for one-shots), and from the live boxes
on the **/learn** page. One scope, everywhere.

---

## 1 · The one idea: everything is a Knot

A **Knot** is one of two things: an **atom** — an arbitrary-precision natural number —
or a **cell** — an ordered pair `[a b]` of two Knots. That's the whole universe.
Text, booleans, and lists are conventions layered on top.

Three surprises are worth internalizing first, because they are pervasive:

1. **`0` means true.** Latte uses *loobean* booleans: `0` is yes, non-zero is no.
   `if` takes its *then* branch when the condition is `0`.
2. **Naturals only.** Subtraction cannot go below zero — it crashes rather than wraps.
3. **Lists end in `0`.** A proper list is right-nested cells terminated by `0`:
   `[1 2 3 0]` is a real 3-element list, but `[1 2 3]` is the pair `[1 [2 3]]`.

Most beginner confusion traces back to one of these three.

## 2 · Atoms and arithmetic

The only primitive Loom provides is the successor, written `+( … )`. Middle-click:

eval +(41)

::  → 42

Everything else — `add`, `sub`, `mul`, `div`, `mod`, `dec` — is defined in the
standard library (`lib/std.lat`) and linked automatically:

eval (add 2 3)

eval (mul 6 7)

eval (div 84 2)

eval (mod 17 5)

::  → 5 · 42 · 42 · 2

A **call is always parenthesized with the function first**: `(add 2 3)` — never
`add(2,3)` or `2 + 3`. Arguments are separated by spaces. And because naturals cannot
be negative, subtraction below zero is an error rather than a silent wrap:

eval (sub 3 5)

::  → error: Bottom("sub jet: underflow")   — by design. `import num` has signed numbers.

## 3 · Loobeans: comparison and logic (0 = true)

Comparisons return a loobean — `0` for "the relation holds":

eval (5 == 5)

eval (5 == 6)

eval (lt 2 3)

eval (gt 2 3)

::  → 0 (yes) · 1 (no) · 0 (yes) · 1 (no)

`==` is built into the language; `lt gt lte gte not and or min max` are library arms.
The inversion feels strange for ten minutes and then becomes second nature.

## 4 · Cells and the cell primitives

Square brackets build cells; four operations are built into the language:

eval head [1 2]

eval tail [1 2 3 0]

eval iscell [1 2]

eval iscell 7

::  → 1 · [2 [3 0]] · 0 (yes) · 1 (no)

Brackets **autocons to the right** — `[1 2 3 0]` is really `[1 [2 [3 0]]]`, which is
why `tail` returned `[2 [3 0]]` above.

## 5 · Lists: right-nested cells terminated by 0

A **list** is autocons plus a convention: end it with `0`. **Always write the trailing
zero** — the standard library's list arms all assume it:

eval (range 5)

eval (len [1 2 3 0])

eval (reverse [1 2 3 0])

eval (append [1 2 0] [3 4 0])

eval (nth [10 20 30 0] 1)

::  → [0 [1 [2 [3 [4 0]]]]] · 3 · [3 [2 [1 0]]] · [1 [2 [3 [4 0]]]] · 20 (0-indexed)

Lists print in their nested form; `[0 [1 [2 …]]]` and `[0 1 2 … 0]` are the same value.

## 6 · Choosing: `if`

Both branches are required; *then* runs when the condition is `0`:

eval if (lt 3 5) then 100 else 200

::  → 100

## 7 · Naming things: `let`

`let name = value in body` binds a name for the length of `body`. One `let` can bind
several names, sequentially — each sees the ones before it:

eval let a = 10, b = (add a 5), c = (mul b 2) in c

::  → 30

## 8 · Short-circuit `and` / `or`

Lazy in their second argument, so they double as guards:

eval (and (lt 1 2) (lt 3 4))

eval (or (gt 1 2) (lt 3 4))

::  → 0 · 0   (both true; the second's right side rescued it)

## 9 · Gates: first-class functions

A **gate** is a function value, `fn [params] -> body` — a closure you can pass to the
higher-order arms:

eval (map (fn [x] -> (mul x x)) [1 2 3 4 0])

eval (filter (fn [x] -> (gt x 2)) [1 2 3 4 0])

eval (foldl (fn [a b] -> (add a b)) 0 [1 2 3 4 0])

::  → [1 [4 [9 [16 0]]]] · [3 [4 0]] · 10

One caution: a `let`-bound gate can **not** call itself — the binding is not in scope
inside its own body. Recursion lives in three places instead: the `loop`/`again`
construct (next section), **module arms** (§14 — arms may call themselves and each
other), and a session **`def`** — which you can make right here. Middle-click these
two lines in order:

def fact [n] = loop with [i = n, acc = 1] : if (i == 0) then acc else again((dec i), (mul acc i)) end

eval (fact 5)

::  → 120 — and `fact` now works from every console, page widget, and text in this session.

(The interaction-net engine is the genuine exception: `latte net` compiles
self-recursive `let` gates to lazily-unrolled Ref nodes.)

## 10 · Iterating: `loop` / `again`

The one looping construct: named accumulators, re-entered in tail position with
`again(…)` — no mutation, each pass rebinds. Fibonacci, iteratively:

eval loop with [a = 0, b = 1, i = 10] : if (i == 0) then a else again(b, (add a b), (dec i)) end

::  → 55

Two rules: `again` must be in tail position, and it only exists inside a `loop`.

## 11 · Dispatching on tags: `case`

A `%name` literal is a *tag* (a cord, §13). `case` matches tags, `_` catches all:

eval case %move of %move -> 111 ; %stop -> 222 ; _ -> 0 end

::  → 111

## 12 · A tour of the standard library

`import std` is automatic under `eval`. The headline arms: arithmetic
(`dec add sub mul div mod pow`), bitwise (`shl shr band bor bxor popcount`),
comparison/logic, lists (`len reverse append nth member range take drop last zip
enumerate`), higher-order (`map filter foldl foldr scanl any all count`), aggregates
(`sum maximum minimum argmax`), stable merge `sort`/`sortby`, association lists
(`aget aput ahas adel`), and cords (`cat catall bytelen bytes frombytes numtext`).

eval (sort [3 1 4 1 5 9 2 6 0])

eval (sortby (fn [a b] -> (gte a b)) [3 1 4 1 5 0])

eval (sum (range 101))

::  → ascending · descending · 5050 (Gauss)

A comparator returns loobean `0` when its first argument should come first.

## 13 · Cords: text is just atoms

A **cord** is a string stored as an atom — the text's bytes, little-endian. `%heart`
is the cord "heart"; because a cord is a number, plain `eval` prints it numerically:

eval %heart

eval (cat %heart %beat)

::  → 500135191912 · a bigger number: concatenation is arithmetic on bytes

To see a cord readably, ask for its bytes, and build text with the cord toolkit:

eval (bytes %hello)

eval (bytelen %hello)

eval (numtext 1234)

::  → [%h [%e [%l [%l [%o 0]]]]] · 5 · the cord "1234"

**Join text with `cat`/`catall`, never `add`.** In the GUI's render results and the
type system's `@t` aura, cords display as text; at the bare prompt you see the atom.

## 14 · Modules: from a session `def` to a compiled core

`def` (§9) is for the session; a **module** is for keeps — and the GUI is the best
place to make one. Middle-click:

System.New greet

A fresh module frame opens. Type this into it (fences are for copying, not clicking):

```
import std
core greet
  twice = fn [x] -> (add x x)
  shout = fn [w] -> (cat w %!)
end
```

Press the frame's **Compile** — the module loads into the *running system*; no
restart. Prove it:

eval (twice 21)

::  → 42, the moment Compile succeeds.

Press **Store** and it persists as `pkg/greet.lat`, loading at every startup. The
rules of the module system: `import` merges arms into one flat namespace (call `add`,
not `std.add`); arms are `name = fn [params] -> body` and may call each other and
themselves; arms aren't first-class (wrap one in a gate to pass it); and shadowing is
later-wins, so a compiled module can override a standard arm while you experiment.
From a shell, `latte eval --lib name=path/file.lat "<expr>"` loads a module file the
same way.

## 15 · A little type checking

The substrate is untyped; two optional layers add safety. The static checker infers a
structural type — `@` (atom), `[T T]` (cell), `*` (unknown) — and flags only provably
wrong operations:

type [1 [2 3]]

type +([1 2])

::  → [@ [@ @]] · type error: `+` expects an atom but got a cell

Runtime **molds** (`import mold`: `bunt`, `nest`, `clam`) validate and coerce values;
`latte mold` in a shell gives the guided tour.

## 16 · Two worked examples, end to end

**Counting primes** — three `def`s that call each other; middle-click each in order:

def isprime [n] = if (lt n 2) then 1 else loop with [d = 2] : if (gt (mul d d) n) then 0 else if ((mod n d) == 0) then 1 else again(+(d)) end

def primes [n] = (filter (fn [x] -> (isprime x)) (range +(n)))

def pcount [n] = (len (primes n))

eval (primes 20)

eval (pcount 100)

::  → [2 [3 [5 [7 [11 [13 [17 [19 0]]]]]]]] · 25

Note the loobean logic inside `isprime`: it returns `0` (prime) when no divisor is
found. When these outgrow the session, `System.New sieve` and paste them into a
`core` — Compile, Store, done.

**Watching an algorithm** — the `algoviz` library instruments classic algorithms and
draws each step through the gfx scene system:

eval (av_steps %bubble [5 2 8 1 9 3 0] 0)

::  → 16 — the trace length; open /learn to scrub the frames under a slider.

## 17 · Gotchas cheat-sheet

- **`0` is true**; `if` runs *then* on `0`.
- **Naturals only**: `(sub 3 5)` crashes; `import num` for signed numbers.
- **Lists end in `0`**: `[1 2 3]` is a pair-of-pairs, not a list.
- **Cords are atoms**: join text with `cat`, never `add`.
- **Calls are parenthesized, function first**, arguments space-separated.
- **Arms and `def`s recurse; bare `let` gates don't** — use `loop`/`again`, a module
  arm, or a `def`.
- **`again` only inside a `loop`, in tail position**; `if` needs both branches.
- A `def` lasts the session; **Compile + Store** makes it permanent.

## 18 · Where to go next

`def` alone lists your session functions; `undef NAME` removes one; `libs` lists
every loaded module. **Using Latte from the GUI** (on this shelf) walks the full
workflow — texts, tools that render, live pages. **The Latte Language** is the
complete reference. The `/learn` page plays algorithms under sliders, `/tools` puts
every tool in a widget, and `lib/*.lat` — the standard library, the type system, the
chess engine — is the best reading in the house: the system you are using is written
in the language you just learned.
