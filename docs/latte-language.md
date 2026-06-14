# The Latte Language

Latte is the functional programming language of the Orpheus environment. Programs are
small, pure, and compile to **Loom** — a 12-rule Nock-style virtual machine whose only
datatype is the **Knot** (an *atom* or a *cell*). The standard library, the type system, the
chess engine, the planner, the Mocha apps, and the system's own command set are all written
in Latte (see `lib/*.lat`).

This document is the language reference. For *writing and registering a library* see
[`adding-libraries.md`](adding-libraries.md); for the markup language that embeds tool calls
see [`facet-language.md`](facet-language.md).

---

## 1. Values

Every Latte value is a **Knot**:

- an **atom** — an arbitrary-precision natural number (`0, 1, 2, …`). There are no negative
  numbers and no floats at the language level; signed fixed-point numbers are a *library*
  (`import num`), and naturals underflow loudly (`(sub 3 5)` crashes rather than wrapping).
- a **cell** — an ordered pair `[a b]` of two Knots.

Everything else is a convention on top of those two:

- **Cords (text).** A short string is an atom holding its bytes little-endian; the tag literal
  `%heart` is the cord `"heart"`. Cords are atoms of at most 16 bytes (they fit in the machine
  word), so they are for tags and short labels, not arbitrary prose.
- **Booleans are loobean: `0` = true, any non-zero = false.** This is the Nock convention and
  it is pervasive: `(5 == 5)` is `0`, `(lt 2 3)` is `0`, and `if` takes the *then* branch when
  its condition is `0`. Read `0` as "yes".
- **Lists are right-nested cells terminated by `0`.** `[1 2 3 0]` is `[1 [2 [3 0]]]`; the empty
  list is `0`. `[1 2 3]` (no trailing `0`) is the *triple* `[1 [2 3]]` — a pair whose tail is a
  pair. Idiomatic lists always end in `0` so that `(xs == 0)` detects the end.

```
[a b]        a cell (pair)
[a b c]      [a [b c]]            (right-nested; a 3-tuple)
[a b c 0]    [a [b [c 0]]]        (a proper 3-element list)
0            the empty list / false-ish / "no"
```

---

## 2. Lexical structure

- **Comments** start with `::` and run to end of line.
- **Numbers**: decimal `42`, hex `0xFF`, binary `0b1010`; underscores are ignored (`1_000`).
- **Identifiers**: start with a letter or `_`, then letters, digits, `_`, or `-`
  (`choose_ml`, `is-prime`).
- **Tags**: `%name` is a cord literal (`%ok`, `%move`).
- **Punctuation/operators**: `[ ] ( ) , ; :`, `->` (gate arrow and case arm), `==` (equality),
  `=` (binding), `+` (used only as `+( … )`, see below).

Whitespace is insignificant except as a token separator. There is no statement terminator; an
expression is the whole program (or arm body).

---

## 3. Expressions

Latte is an expression language: every construct yields a value.

### Literals and references
```
42            number literal
%tag          cord literal
nil           the atom 0 (handy name for the empty list)
xs            a variable (a parameter, a let-binding, or a loop binding)
```

### Cells and lists
Square brackets build right-nested cells (autocons):
```
[1 2]                 a pair
[ (head xs) acc ]     cons: prepend onto a list (acc is the rest)
[1 2 3 0]             a 3-element list
```
A single-element `[x]` is just `x` (grouping); empty `[]` is an error.

### The cell primitives
These are built into the language (not library calls):
```
head c        the head (left) of a cell
tail c        the tail (right) of a cell
iscell x      0 if x is a cell, 1 if it is an atom (loobean)
+( e )        increment: the successor of e   ( +(41) = 42 )
(a == b)      structural equality: 0 if equal, 1 if not (loobean)
```

### Function application
A call is always parenthesised, with the function name first:
```
(add 2 3)              call the arm/gate `add` with two arguments
(map (fn [x] -> +(x)) xs)
```
`(e)` with no following arguments is just grouping. `( e == e )` is equality. Everything else
inside parentheses starting with a name is a call.

### Conditional
```
if (cond) then A else B
```
`A` is taken when `cond` evaluates to `0` (loobean true), `B` otherwise. Both branches are
required.

### Let
```
let name = value in body
```
Binds `name` for the duration of `body`. Lets nest freely:
```
let a = (head p) in
let b = (tail p) in
(add a b)
```
A single `let` may bind several names at once, separated by commas; the bindings
are sequential (each sees the ones before it), which keeps a deep pipeline readable
instead of a tower of nested `let`s:
```
let a = 10, b = (add a 5), c = (mul b 2) in c   :: 30
```

### Short-circuit `and` / `or`
```
(and a b)     :: 0 (true) iff both are true; b is evaluated only if a is true
(or  a b)     :: 0 (true) iff either is true; b is evaluated only if a is false
```
These are lazy in their second operand (they desugar to `if`), so a guard never
evaluates an unsafe right-hand side:
```
(and (gt n 0) (safe (sub n 1)))   :: (safe ..) runs only when n > 0
```

### Case (tag dispatch)
Matches a value against cord tags, with `_` as the default; arms are separated by `;` and the
block ends with `end`:
```
case tag of
  %set   -> (ok arg) ;
  %clear -> (ok 0) ;
  _      -> (ok state)
end
```

### Loop / again (tail recursion)
The only iteration construct. `loop with [bindings] : body end` introduces named accumulators;
`again(…)` re-enters the loop with new values for them, in declaration order:
```
fib = fn [n] ->
        loop with [a = 0, b = 1, i = n] :
          if (i == 0) then a
          else again(b, (add a b), (dec i))
        end
```
`again` must appear in tail position. A loop that never calls `again` simply returns its body.

### Gates (first-class functions / closures)
`fn [params] -> body` is a closure value. It captures the surrounding environment and can be
passed to higher-order arms:
```
(filter (fn [x] -> (isprime x)) (range 100))
(foldl (fn [a b] -> (add a b)) 0 xs)
```
Parameters are destructured positionally; a one-parameter gate `fn [x] -> …` receives the whole
argument, a two-parameter gate `fn [a b] -> …` receives a pair, and so on.

### Jet hints
```
fast %add  <body>
```
`fast %name` annotates a body with a *jet* hint: if the host has a native implementation
registered under `%name` (an audited fast path), it runs that instead of interpreting the body,
but the body remains the ground-truth definition. This is how `lib/std.lat` makes arithmetic
fast without leaving the language. You rarely write `fast` yourself.

---

## 4. Modules

A `.lat` file is an optional list of `import`s followed by one `core` block of **arms**
(named gates):

```
:: greet.lat
import std
core greet
  twice = fn [x] -> (add x x)
  bump  = fn [x] -> (add (twice x) 1)
end
```

- **`import NAME`** links another module's arms into scope. Imports are resolved recursively
  and merged into one flat namespace, so an imported arm is called by its bare name (`add`,
  not `std.add`).
- **`core NAME`** opens the module; `NAME` is documentation (the registry key when the module
  is loaded). `end` closes it.
- **Arms** are `name = fn [params] -> body`. A trailing `;` between arms is optional.
- **Arms are not first-class values.** You cannot pass `add` itself as an argument; wrap it in a
  gate: `(foldl (fn [a b] -> (add a b)) 0 xs)`. Gates *are* first-class.
- **Shadowing.** When linked modules define the same arm name, the later one wins. The default
  GUI/console scope links every loaded library, so a freshly compiled module can override a
  built-in arm.

---

## 5. The standard library (`import std`)

`lib/std.lat`, written in Latte over the single Loom successor primitive:

- arithmetic on naturals: `dec add sub mul div mod` and `pow`
- bitwise on naturals: `shl shr bit lowbit popcount band bor bxor` (shifts are exact `×/÷ 2^k`;
  AND/OR/XOR fold bit by bit). The data-intensive libraries use these for Bloom-filter bitsets
  and zigzag varints.
- comparison / logic (loobean): `lt gt lte gte not and or min max`
- lists: `len reverse append nth member range`, `take drop`, and stable `sort` / `sortby`
  (merge sort; a `sortby` comparator returns the loobean `0` when its first argument sorts first)
- higher-order: `map filter foldl foldr`
- cords (strings): `bytelen cat catall`, plus `bytes` (cord → low-first byte list) and
  `frombytes` (byte list → cord) — the basis of the string-algorithms library

Other built-in libraries: `mold` (the aura/type system), `num` (signed fixed-point), `tensor`,
`ml`, `plan`, `plot`, `vec`, `chess`, `chessml`, `tool` (the system command set), `mocha` (the
app runtime). Link any of them with `import`; in the GUI console and `eval` they are all in
scope already.

---

## 6. Running Latte

- **One-off expression:** `latte eval "(mul 6 7)"` → `42`. The `eval` path links the whole
  standard scope, so `(fib 10)`, `(primes 30)`, `(tsum …)` all resolve.
- **REPL:** `latte repl` (self-hosting environment) and `latte cli` (`eval`, `:type`, `:rust`,
  `:libs`).
- **GUI console:** at `/` (the System page), a `Module.command args` line runs the arm
  `command`; bare `eval/type/sca/…` verbs are also accepted. See the GUI section of the README.
- **Compile a module into the running system (Oberon-style):** `POST /api/compile` with a
  `core NAME …` body registers it live — no binary rebuild. From the GUI, open a module
  (`System.Open NAME`), edit, and run `Compiler.Compile *`.
- **Native compilation (Anvil):** `latte rustc "<expr>"` compiles a Latte expression to Rust;
  the GUI `eval` path uses the same compiler with a cache, falling back to the interpreter.

---

## 7. A worked example

A module that counts the primes below `n`, compiled and called live:

```
import std
core sieve
  isprime = fn [n] ->
              if (lt n 2) then 1
              else loop with [d = 2] :
                     if (gt (mul d d) n) then 0      :: no divisor ⇒ prime (0 = yes)
                     else if ((mod n d) == 0) then 1 :: divisible ⇒ composite
                     else again(+(d))
                   end
  primes  = fn [n] -> (filter (fn [x] -> (isprime x)) (range +(n)))
  count   = fn [n] -> (len (primes n))
end
```
`(count 100)` → `25`. (`lib/tool.lat` ships exactly these as `Tool.primes` / `Tool.countprimes`.)

---

## 8. Gotchas

- **Loobean truth is inverted from most languages.** `0` is true. `(a == b)` is `0` when equal.
  `if (cond) then …` runs *then* on `0`.
- **Naturals only.** Subtraction underflows into a crash; use `import num` for signed values.
- **Lists end in `0`.** `[1 2 3]` is *not* a 3-list — it is `[1 [2 3]]`. Write `[1 2 3 0]`.
- **Cords are ≤ 16 bytes.** Use lists of cords for longer text.
- **Arms aren't values; gates are.** Wrap an arm in `fn […] -> (arm …)` to pass it around.
- **`again` only inside a `loop`, in tail position.**
