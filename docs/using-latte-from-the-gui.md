# Using Latte from the GUI — a tutorial

Latte is not only a language you run from a shell; it is the working material of the
Orpheus GUI. This tutorial walks from a first evaluation to a compiled, persistent,
rendering tool of your own — without leaving the browser. Everything here is live:
each step's result comes from the running system, and each artifact you make joins it.

Start the system and open it:

```
latte gui            # serves the GUI; open the printed address (127.0.0.1:8088)
```

The desktop is texts in viewers, the Oberon way. The **System** viewer's command
line is where we begin; the Log viewer beside it collects results.

## 1 · Your first evaluations

Type into the System command line:

```
eval (add 2 3)
eval (map (fn [x] -> (mul x x)) [1 2 3 4 0])
eval (sort [3 1 4 1 5 9 2 6 0])
```

Three things to know, and most confusion evaporates: **`0` means true** (a loobean —
`eval (lt 2 3)` prints `0` because yes); **lists end in `0`** (`[1 2 3 0]` is a
three-element list, `[1 2 3]` is nested pairs); and **a call is parenthesized with
the function first** — `(add 2 3)`, arguments separated by spaces.

The command line is nothing special. **Any command line in any text runs** —
middle-click it (or put the caret on it and press Ctrl/⌘+Enter). A tool's printed
output, a document you are writing, this tutorial open in a viewer: if a line reads
like a command, pointing at it executes it. Texts are the interface.

## 2 · Define functions from any text — `def`

A one-liner turns into a session function:

```
def sq [x] = (mul x x)
eval (sq 12)                          → 144
def cube = fn [x] -> (mul x (sq x))   :: the explicit gate form; may call earlier defs
eval (cube 4)                         → 64
def                                   :: lists the session's functions
undef sq                              :: removes one (dependents are warned about)
```

Definitions accumulate in a `user` module compiled through the same validating path
as every other module — a broken `def` is rejected and the previous good set stays.
Because `user` joins the standard scope, a defined function is callable from `eval`
anywhere: the console, a Facet page's widgets, the CLI, the HTTP API.

Two companions while you are here:

```
type (fn [x] -> [x x])   :: infer an expression's type
libs                     :: list every module loaded in the running system
```

## 3 · From `def` to a module of your own

`def` is for the session; a **module** is for keeps. Open a fresh module frame:

```
System.New greet
```

Write a module — imports first, then one `core` of arms:

```
import std
core greet
  twice = fn [x] -> (add x x)
  shout = fn [w] -> (cat w %!)
end
```

The frame's menu has **Compile**, **Store**, and **Format**:

- **Compile** loads it into the *running system* immediately — the very next command
  line can `eval (twice 21)`. No restart, no rebuild: this is the Oberon loop.
- **Store** persists it. A name not shipped in `lib/` writes to `pkg/greet.lat`,
  and packages load automatically at every startup — Compile + Store is a permanent
  extension of the system.
- **Format** runs the compile-checked source formatter.

The Modules viewer lists every module (`·live` marks runtime-compiled ones); click a
name to open its source. Shadowing is later-wins, so a module you compile can even
override a standard arm while you experiment.

## 4 · Tools that render

An arm that returns a tagged cord **embeds as a live object** where the command ran:
`[%html <cord>]` for markup, `[%svg <cord>]` for graphics. The `hello` module,
compiled at boot, is the working demonstration:

```
hello.badge 9
hello.spark [3 [9 [5 [12 [7 0]]]]]
```

Build text with the cord toolkit (`cat`, `catall`, `numtext`, `fixtext`, string
literals `"like this"`), and remember the arithmetic is arbitrary-precision — a
kilobyte of markup is just a very large atom, and building one is safe. `ui Tool.arm`
wraps any such tool in an interactive panel.

For algorithmic drawing at a higher level, return a **gfx scene** (a list of
`(rect …)`, `(circle …)`, `(text …)` shapes — `import gfx`) and the system's one SVG
renderer draws it; that is exactly how the `/learn` page's algorithm animations work
(`import algoviz`).

## 5 · Latte on live pages

The hosted pages evaluate Latte server-side and stay interactive:

- **`/learn`** — the interactive tutorials: the language in four editable boxes,
  algorithms played under a slider.
- **`/tools`** — every environment tool as a live widget, including
  `Latte.eval(expr)`: an editable box whose result re-evaluates as you type.

In your own Facet page, one hole makes Latte interactive:

```
{{ Live.box("Latte.eval(expr)", [["expr", "(sum (range 101))"]]) }}
```

Edits re-evaluate on the server and swap the result in place. Since the scope-core
cache landed, a fresh expression evaluates in well under a millisecond, so the box
follows your typing.

## 6 · The engines, and knowing which one you are on

Two engines produce identical results (a differential fuzzer enforces it):

- **Anvil** compiles an expression to native code via `rustc`, content-addressed and
  cached — heavy code lands here.
- **The interpreter** (Loom, jet-accelerated) runs everything else and is the
  always-correct fallback.

The GUI's `eval` chooses adaptively, and the choice is **measured, not guessed**:
interpreter runs are timed, and a program that proves slow is compiled automatically
before its next run. From a shell, `latte profile "<expr>"` shows both engines'
timings and the decision; `latte cache path` inspects the compiled-program cache.

## 7 · Five minutes, end to end

1. `eval (sum (range 101))` — first result in the Log.
2. `def fib [n] = loop with [a = 0, b = 1, i = n] : if (i == 0) then a else again(b, (add a b), (dec i)) end`
   then `eval (fib 30)` → `832040`.
3. `System.New numbers`, write a `core numbers` with `fib` and a
   `card = fn [n] -> [%html (catall [ "<b>fib " (numtext n) " = " (numtext (fib n)) "</b>" 0 ])]`
   arm — **Compile**, then run `numbers.card 20` and watch it embed. **Store** it;
   it now loads at every startup.
4. Open `/learn` and type your `(fib 30)` into the language boxes — the same scope,
   the same answer, from a page.

That is the whole loop: evaluate, define, compile into the running image, render,
persist. The system you are using is the system you are extending.

## Where to go next

**The Latte Tutorial** (on this shelf) teaches the language itself — and it is
executable: opened in the Docs viewer, its examples are command lines you
middle-click. `docs/latte-language.md` is the full language reference; `docs/the-system.md`
covers viewers, marking, and the command conventions; `docs/adding-libraries.md`
shows how a library graduates into the shipped set; and the `/tools` page keeps
every tool one widget away.
