# Adding a New Library to Latte

A Latte library is a small `.lat` file that defines a named module of functions. Libraries
build on one another with `import`, run on the Loom VM, and can optionally be accelerated with
native *jets*. This guide walks through writing one, registering it so `import` can find it,
testing it, and wiring it into the CLI and GUI — ending with a complete worked example.

---

## 1. Anatomy of a library

A library is an `import` list followed by a single `core` block of definitions:

```
:: vec.lat — a tiny vector library
import std
core vec
  :: sum of a 0-terminated list
  vsum = fn [xs] -> (foldl (fn [a b] -> (add a b)) 0 xs)  :: add is wrapped: module funcs are not first-class

  :: dot product of two equal-length lists (prefixed names avoid clashes)
  vdot = fn [xs ys] ->
          loop with [a = xs, b = ys, acc = 0] :
            if (a == 0) then acc
            else again((tail a), (tail b), (add acc (mul (head a) (head b))))
          end
end
```

Key conventions, all enforced by the compiler:

- **Every definition is `name = fn [params] -> body`.** Module functions are *not*
  first-class values, so to pass one around (e.g. to `map`) you wrap it: `(map (fn [x] -> (add x 1)) xs)`.
- **Calls are parenthesized:** `(vdot xs ys)`, never `vdot xs ys`. `+(e)` is increment;
  `again(..)` re-enters the nearest `loop`.
- **Lists are 0-terminated cons cells.** `[a b c 0]` right-nests to `[a [b [c 0]]]`; the empty
  list is `0`. Iterate by testing `(xs == 0)` and recursing on `(tail xs)`.
- **Loobean truth is 0 = true, 1 = false** (so `(a == b)` returns 0 when equal). `if` follows
  this: `if (cond) then ... else ...`.
- **`import std`** gives you `add sub mul div mod lt gt … len reverse append nth map filter
  foldl range` and more. Importing a library transitively imports what it imports.

---

## 2. Registering the library

`import NAME` resolves through a small built-in registry in `src/latte.rs`. To make your
file importable, add it there in two places:

```rust
// 1. embed the source at compile time
pub const VEC_LAT: &str = include_str!("../lib/vec.lat");

// 2. add a match arm in builtin_lib(name)
"vec" => Some(VEC_LAT),
```

After that, `import vec` works inside other `.lat` files, and from Rust you can run an
expression against it:

```rust
let n = latte::run_with_libs("(vdot [1 [2 [3 0]]] [4 [5 [6 0]]])", &["std", "vec"])?;
```

Libraries linked together **share one namespace** — every `core` block's definitions merge.
So pick names that won't collide. The numeric stack uses prefixes for exactly this reason
(`num` exposes `nadd/nmul/…`, `tensor` exposes `tadd/tdot/…`); `num` even renamed its `scale`
to `nscale` so it could be linked alongside `plan`'s `scale`. When in doubt, prefix. (On a
name clash the *last* library linked wins — definitions shadow silently rather than erroring.)

**Every built-in library is loaded by default.** `latte eval`, the REPL, and the GUI console
all link the full set returned by `latte::all_libs()` (std, mold, mocha, plan, num, tensor, ml,
plot, vec, chess, chessml, plus anything registered at run time), so you can call `vdot`,
`nadd`, `vsum`, … with no `import` at all. Library *gathering* (parse + import resolution) is
cached the first time each library is used, so repeated calls don't re-parse the standard
libraries — the libraries are effectively compiled once and reused.

---

## 2b. Loading a library at run time (no rebuild)

Built-in registration (above) bakes a library into the binary. You can also add one to a
**running** system — the Oberon way: edit a module and compile it into the live image.

- **From the CLI**, point `--lib NAME=FILE` at a `.lat` file (repeatable):

  ```
  latte eval --lib widgets=lib/widgets.lat "(area 6 7)"
  ```

- **Over the network**, `POST /api/lib` with a body of `NAME\n<source>` registers it on the
  server; `POST /api/compile` with just a module body compiles it, reports any errors with
  line/column, and (on success) loads it — this is what the GUI **Compiler** page
  (`/compile`) does. After either, the whole system can `import NAME` or call it in the console.

- **From Rust**, `latte::register_runtime_lib(name, src)` registers a source string, and
  `latte::compile_and_register(src)` compiles-and-loads a `core NAME …` module, returning a
  status message. `import` consults the runtime registry *before* the built-ins, so a runtime
  library can also shadow a built-in of the same name.

Runtime libraries may themselves `import` other libraries (runtime or built-in).

---

## 3. (Optional) Make it fast with a jet

Pure-Latte code runs on the interpreter. For hot inner loops you can mark a definition with a
`fast %name` hint and register a native Rust implementation (a *jet*) under that name:

```
add = fast %add (fn [a b] -> ...slow reference version...)
```

The reference body still defines the meaning; the jet just runs in its place when jets are
enabled. Jets live in `src/jets.rs`: each reads its argument out of the subject at the sample
axis and returns a `Knot`. Register it in `register_std_jets()`. Because the reference version
remains, you can run in *audit mode* to check the jet against it on every call — the same way
the arithmetic jets are validated. Keep jets for genuinely hot paths (arithmetic, comparisons);
most library code does not need them.

---

## 4. Testing it

Add a Rust test that runs expressions against your library and checks the resulting noun.
The numeric libraries (`src/numerics.rs`) and the visualization host (`src/viz.rs`) are good
templates:

```rust
#[test]
fn vec_dot_product() {
    let n = latte::run_with_libs(
        "(vdot [1 [2 [3 0]]] [4 [5 [6 0]]])",
        &["std", "vec"],
    ).unwrap();
    assert_eq!(n.as_atom().unwrap().to_u128().unwrap(), 32);
}
```

If your library does numeric work, you can also **audit it against an independent oracle** —
this is exactly how the interaction-net arithmetic is checked against the Loom interpreter in
`src/icomb.rs`: compute the answer two different ways and assert they agree across a range of
inputs.

---

## 5. Exposing it to users

A library is usable from `latte eval` and the GUI console as soon as it is linked into the
expression's library set. Two common ways to surface it:

- **A CLI demo command.** Add a `cmd_vec(args)` (see `numerics::cmd_tensor` for the pattern)
  and a dispatch arm in `src/main.rs`: `"vec" => vec_host::cmd_vec(&args[1..])`.
- **The System console.** The console's `eval` already links the standard library set in
  `run_tool` (`src/serve.rs`). Add your library name to that set and any
  `(vdot …)` call typed into the Commander will resolve. You can also add a starter button to
  `lib/site/system.html`.

For a brand-new *visual* tool, follow `viz.rs` + `/api/plot` + `chart.html`: compute the data
or layout in Latte, serialize/format it in a thin Rust host, expose an endpoint, and add a page.

---

## 6. Worked example, end to end

1. **Write** `lib/vec.lat` (the file in §1).
2. **Register** it in `src/latte.rs`: add `VEC_LAT` and the `"vec"` arm.
3. **Build:** `cargo build --offline --release`.
4. **Try it:** `latte eval "(vdot [1 [2 [3 0]]] [4 [5 [6 0]]])"` → `32` (all libraries are in
   scope by default, so no manual import is needed). Or, without rebuilding at all, load it at
   run time: `latte eval --lib vec=lib/vec.lat "(vdot …)"`.
5. **Test:** add `vec_dot_product` and run `cargo test --offline`.
6. **Ship:** refresh the distribution so the rebuilt binary is packaged.

That is the whole loop. Most libraries are a single small `.lat` file plus a two-line registry
edit; jets, hosts, and GUI pages are optional layers you add only when you need speed or a
dedicated front end.

---

## Checklist

- [ ] `import` what you use; one `core NAME` block; every def is `name = fn [..] -> ..`.
- [ ] 0-terminated lists; parenthesized calls; Loobean 0 = true.
- [ ] Names are unique across the libraries you link together (prefix to avoid clashes).
- [ ] Registered in `builtin_lib` (`const` + match arm) so `import` finds it.
- [ ] A test that runs an expression and checks the noun; an oracle cross-check if numeric.
- [ ] Need negatives or fractions? build on `num` (signed fixed-point); n-d data? on `tensor`.
- [ ] Optional: a jet for hot paths, a `cmd_` + dispatch for the CLI, a console entry / page.

## Where things live

| file | role |
|------|------|
| `lib/<name>.lat` | the library source |
| `src/latte.rs` | `builtin_lib` registry (`const` + match arm) |
| `src/jets.rs` | native jets and `register_std_jets()` |
| `src/numerics.rs`, `src/viz.rs` | example hosts (CLI demos, tests, rendering) |
| `src/main.rs` | CLI dispatch (`cmd_…`) |
| `src/serve.rs`, `lib/site/*.html` | GUI console (`run_tool`) and pages |
| `src/latte.rs` · `register_runtime_lib` / `compile_and_register` / `all_libs` | run-time library loading, Oberon-style compile, default scope |
| `/api/lib`, `/api/compile`, `lib/site/compile.html` | load / compile a library from the GUI at run time |

## The package system, transparently

There is no package format, no manifest, no registry daemon. **A package is a
Latte source whose `core NAME` names it** — the same thing a library is. The
whole mechanism:

1. `import NAME` resolves in three tiers: modules compiled at run time (the
   live registry), then the built-in libraries (`lib/*.lat`, compiled into the
   binary), then **`pkg/NAME.lat`** — the user package directory.
2. Compiling a module — `Compiler.Compile` in a GUI frame, or just dropping a
   `.lat` file into `pkg/` — registers it in the running system at once: the
   next command can call it, `libs` lists it, the Modules viewer marks it
   `·live`.
3. `Store` persists it. Names that ship in `lib/` write back there (you are
   editing the system); anything else writes `pkg/<name>.lat`. Everything in
   `pkg/` is compiled and registered automatically at startup, so Compile +
   Store is a permanent extension.
4. `latte pkg` prints the inventory; `latte fmt file.lat --write` formats a
   source (conservatively — the result is compile-checked, and the original is
   returned untouched if formatting would break it).

This answers "where does Latte code that is not library code live": **your
modules live in `pkg/`, your documents-with-objects live in `text/`,** and the
system's own libraries live in `lib/`.
