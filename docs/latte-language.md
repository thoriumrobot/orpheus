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

- **Cords (text).** A string is an atom holding its bytes little-endian; the tag literal
  `%heart` is the cord `"heart"`. Cords can be any length — short ones fit a machine word, longer
  ones are carried as byte-vector atoms — so the cord operations (`cat`, `bytelen`, `bytes`, …)
  handle tags, labels, and arbitrary text alike, on both the interpreter and the native backend.
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
  the `eval`/GUI paths use the same compiler with a content-addressed binary cache (keyed by a
  hash of the emitted source), falling back to the interpreter when a program is outside the
  native subset. Atoms are `u128` for numbers and byte-vector `Big` atoms for cords of any
  length, so string-heavy code compiles natively; binders that collide with Rust keywords or
  jetted cord ops are alpha-renamed.
  - **Build tuning.** Native builds use `rustc -C opt-level=0` by default: for these one-shot,
    cached programs the compile dominates the sub-second runtime, so opt-0 cuts cold-start ~7–9x
    (≈0.9s vs ≈6.3s on the 614-rule Ligurian `evolve`) while still running several times faster
    than the interpreter. Set `ORPHEUS_OPT=2` (or `3`) for hot, long-lived programs where runtime
    dominates. `ORPHEUS_CACHE` relocates the cache directory.
  - **Compile once, feed many inputs.** A program can take its input at run time (the expression
    references the bound parameter `__in`); the binary is content-addressed by the *program*, not
    the input, and reads each input from stdin. So a heavy program is built once and reused across
    all inputs (e.g. the whole Ligurian evolution: one ~0.9s build, then ~50ms per word).
  - **Cache control.** `latte cache` (status: count, size on disk, opt level, size cap), `latte
    cache warm "<expr>"` (prebuild so a later run is instant), `latte cache clear` (reclaim disk —
    the cache is purely derived, so this is always safe). The cache **self-bounds**: it stays under
    a size cap (default 512 MiB, set via `ORPHEUS_CACHE_MAX` in MiB, `0` disables) by evicting the
    least-recently-used binaries after each build. Recency is the binary's mtime, refreshed on every
    run, so hot programs (e.g. the shared `evolve` binary) survive while stale one-offs are reclaimed.
  - **Differential fuzzing.** The backend's safety rests on *native == interpreter on success*, so
    a seeded fuzzer stress-tests exactly that: it generates random well-formed, terminating programs
    from the native subset — type-aware (arithmetic operands are atoms, partial ops guarded, so most
    programs run rather than hit domain errors) and covering the hard paths: closures with
    free-variable capture, higher-order functions (`map`/`filter`/`foldl`/`any`/…), list operations,
    `case`, and bounded loops. It asserts **soundness** — if the native binary yields a value, the
    interpreter yields the same value; native *declining* (a legitimate fallback like u128-overflow
    arithmetic) is allowed. A modest seeded run is part of the test suite; `latte anvil fuzz <iters>
    <seed>` runs an extensive, reproducible campaign as a release gate. On a divergence it
    **minimizes** the offending program to a small reproducer (greedy structural shrinking: collapse
    each balanced subterm to `0`, or hoist it to be the whole program — every candidate re-checked,
    so the reproducer is always genuine and smaller). The same shrinker powers `latte anvil shrink
    "<expr>"`, which reduces any program to its smallest *non-native* subterm (a bug reproducer, or a
    "what here falls back?" probe). Across hundreds of generated programs the fuzzer has found no
    divergence.
  - **Build metrics.** Anvil keeps lifetime counters (persisted in the cache dir, so they span
    one-shot CLI runs): `rustc` builds and their total/average time, cache hits, shared-store pulls,
    and build failures. `latte cache metrics` shows them with a reuse rate and an estimate of `rustc`
    time saved (hits + pulls × average build time); `latte cache status` carries a one-line summary.
    Counters are advisory — approximate under heavy concurrency, and a host that only ever pulls
    reports no time saved since it has no local build to measure against.
  - **Diagnosing fallbacks.** When a program can't run natively it falls back to the interpreter
    (correct, but slower). To make that visible rather than silent: `latte eval --explain "<expr>"`
    reports whether the program compiles to native code and, if not, why (e.g. `unknown function or
    gate 'foo'`, `'add' expects 2 args, got 1`). The rarer case — a program that lowers to Rust but
    `rustc` rejects (a codegen bug) — is captured to a bounded log; `latte cache log` shows the most
    recent build failures with the compiler's own error text instead of swallowing them.
  - **Integrity & self-healing.** Every built binary gets a sidecar (`<name>.sha` = `sha3` + size)
    recorded by its producer. Before running a cached binary Anvil does a near-free size check and,
    on a mismatch (truncation, an interrupted write), purges and rebuilds it automatically. Pulls
    from the shared store are *fully* hash-verified before install, so a corrupt or poisoned store
    entry — even one that preserves the original size — is rejected rather than infecting the host.
    `latte cache verify [--repair]` audits the whole local cache by full hash and (with `--repair`)
    purges anything corrupt so it rebuilds on next use. Full hashing is paid only on pull and on this
    explicit command, never on the hot run path.
  - **Shared build store (across hosts).** Set `ORPHEUS_CACHE_SHARED` to a directory (an NFS mount,
    a synced folder, a CI cache) and the local cache becomes a read-through/write-back mirror of it:
    on a local miss Anvil first looks in the shared store and, on a hit, copies the binary in and
    skips `rustc` entirely; after a local build it publishes the result back. So each distinct
    program is compiled once *across the whole fleet* rather than once per host. Artifacts are
    namespaced by toolchain identity (`rustc` release + host target triple), so a host never pulls a
    binary it can't run; builds use no `target-cpu=native`, keeping codegen portable across CPUs of
    the same triple. Every shared-store operation is best-effort — any error falls through to a
    normal local build, so the feature can never break a build, only accelerate it.
  - **Signed networked artifact registry.** For a fleet without shared storage (CI runners, separate
    machines), `latte anvil registry serve [addr] [root]` runs an HTTP service (on the same shared
    server core as the Hymn web server) that holds compiled binaries. Point hosts at it with `ORPHEUS_REGISTRY=http://host:port`: on a local miss
    Anvil pulls the binary over the network (after the shared store, before `rustc`), and after a
    local build it publishes back — so, like the shared store, each program compiles once across the
    fleet, but over HTTP instead of a shared filesystem. Because artifacts now cross an untrusted
    network they are **signed**: every binary carries an HMAC-SHA3-256 MAC keyed by
    `ORPHEUS_REGISTRY_KEY`. The server refuses to store an upload whose MAC doesn't verify (`401`),
    and a client refuses to *install* a download whose MAC doesn't verify — so a tampered or
    unauthenticated artifact is rejected rather than executed. Artifacts are namespaced by toolchain
    identity exactly like the shared store, the store is kept bounded by size-capped LRU eviction
    (`ORPHEUS_REGISTRY_MAX`, MiB), and a server holding the key re-verifies each artifact's MAC on
    read so on-disk corruption is caught rather than served. This is a shared-key MAC (integrity + authenticity among
    parties holding the key — the trusted-CI-builders model), deliberately *not* a public-key
    signature: a real asymmetric scheme would need a vetted crypto library, which has no place in a
    hand-rolled zero-dependency codebase. Transport is plain HTTP for a trusted network/loopback; all
    operations are best-effort, falling through to a local build on any error.
  - **Resident compile server (`anvild`).** `latte anvil serve` starts a small daemon (a Unix
    socket under the cache dir) whose one job is *background compilation that outlives a one-shot
    client*. With it running, a cold `latte eval`/`latte evolve` doesn't stall on `rustc`: the
    program already on disk runs in-process; a cold one is handed to the daemon (which builds it in
    the background) while this call is answered on the interpreter, so the next invocation — even a
    separate process — finds the binary warm. It's opt-in (nothing starts it automatically) and
    shares the same on-disk cache, so it never changes results, only when the build happens.
    Commands: `latte anvil serve | ping | stop | stats | warm "<expr>"`. The trade-off is honest:
    deferring to the interpreter for the current call is a clear win when interpreting is cheap, and
    roughly a wash for a heavy program whose interpreter run rivals its build time — the lasting
    benefit is that every later call is native without a foreground build. The same daemon backs the
    system-wide adaptive policy (`run_adaptive`): every in-process surface that evaluates Latte — the
    GUI console and pages, the data-intensive demos, the database service — runs **heavy code
    compiled, light code interpreted**, and when the daemon is up, a cold heavy program is warmed
    through it without stalling the request. Warm requests carry their library scope, so the daemon
    builds the exact binary the caller will look up (a program needing non-default libraries warms
    correctly, not under the wrong key).

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
- **Cords are atoms (numbers).** Integer ops treat a cord as its little-endian byte value, so
  `(add "ab" 1)` does byte arithmetic, not concatenation — use `cat` to join text.
- **Arms aren't values; gates are.** Wrap an arm in `fn […] -> (arm …)` to pass it around.
- **`again` only inside a `loop`, in tail position.**
