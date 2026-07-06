# The Facet Language

Facet is the markup language of the Orpheus environment — the language the hosted pages are
written in (`lib/site/index.facet`). A Facet document is ordinary markup (any HTML passes
through untouched) with two kinds of **holes**, both written in double braces, that call into
the system's tools and fill the page with their output.

Facet pages are **rendered on the server** by Hymn, on every request, so a page is a
deterministic function of its tool outputs: edit the source and the rendered HTML changes with
no build step. Expressions are pure and side-effect-free — the same philosophy as
[Latte](latte-language.md), which is what the tools behind the holes are written in.

---

## 1. The two holes

```
{{ EXPR }}                         evaluate a Facet expression, insert its text
{{each NAME in EXPR}} … {{end}}    repeat the enclosed template over a list
{{if EXPR}} … {{else}} … {{end}}   conditionally include a template
```

Everything outside braces is copied verbatim, so Facet sits inside HTML naturally:

```html
<h2>Heart Speech</h2>
<ul>
{{each w in ["ligā", "nīvō", "mazdā"]}}
  <li>{{ w }} → {{ SCArs.evolve(w) }}</li>
{{end}}
</ul>
```

`{{else}}` is optional; an `{{if}}` with no `{{else}}` simply emits nothing when false. `each`
and `if` blocks nest, and must be closed with `{{end}}`.

---

## 2. Expressions

Facet expressions are a small functional language:

| Form | Example | Meaning |
|------|---------|---------|
| string | `"zelā"` | a text literal (double quotes) |
| number | `42` | a number literal |
| variable | `w` | a binding from `each` or `let` |
| list | `["a", "b", "c"]` | a list of values (comma-separated) |
| let | `let n = SCArs.evolve("ān") in n` | bind a name within an expression |
| tool call | `SCArs.evolve("zelā")` | call a system tool `Module.procedure(args)` |

The **tool call** is the heart of Facet: `Module.procedure(arg, …)` is exactly the Oberon
`Module.Procedure` convention used throughout the system. This is the single seam through which
a page reaches the running tools.

### Truthiness (for `{{if}}`)
A value is **true** when it is non-empty / non-zero:

- text → true when the string is non-empty,
- number → true when non-zero,
- list → true when it has at least one element.

> Note this is the *opposite* of Latte's loobean (where `0` is true). Facet `if` uses ordinary
> "non-empty is true" truthiness because it operates on rendered tool output, not Loom nouns.

---

## 3. The tool registry

Every tool is an entry in a single **registry** in `src/facet.rs`: a `ToolSpec` records the
module, procedure, signature, a one-line summary, and the handler function; a `ModuleSpec`
records each module's name and summary. `dispatch_tool` routes a `Module.procedure` call to its
handler, and — because the registry is data — the environment can describe *itself* (see the
`Meta` module below), and a page can never document a tool that isn't actually callable.

The shipped modules (44 tools in all; ask `Meta.modules()` / `Meta.tools(m)` for the live list):

| Module | What it offers |
|--------|----------------|
| `SCArs` | the sound-change applier — `pie`, `evolve`, `apply(word, rule…)`, `trace` |
| `Txt`   | composable text helpers — `upper`, `lower`, `cap`, `rev`, `trim`, `len`, `words`, `replace`, `join`, `split`, `esc` |
| `Latte` | the whole Latte language, evaluated live — `eval(expr)` |
| `Viz`   | render data as SVG charts — `chart(kind, numbers)` (bars / line / scatter) |
| `Mkt`   | a market-data lab over a built-in price series — `span`, `stat`, `series`, `chart`, `vol`, `advice`, `forecast` (HAR-RV) |
| `Date`  | civil date arithmetic — `add`, `between`, `weekday`, `ordinal`, `fromordinal` |
| `Sent`  | text sentiment scoring — `polarity`, `label`, `counts` |
| `Hash`  | SHA3-256 digests — `sha3`, `short` |
| `Phono` | phonology — `presets`, `inventory`, `words`, `report`, `coin` |
| `Meta`  | the environment describing its own tools — `modules`, `tools(module)`, `count` |
| `Live`  | live, client-updating widgets — `box(expr, fields)`, `view(expr, fields)` (see §4) |

Tools that produce SVG or HTML (`Viz.chart`, `Mkt.chart`) return raw markup, which renders
inline because holes are not re-escaped. Everything runs on the same VM as the rest of the
system — e.g. `Latte.eval` is the full language, and `SCArs.*` runs `lib/sca.lat` on the Loom —
so a Facet page genuinely computes its content rather than templating canned strings.

### Adding a tool
Write a handler `fn(&[Val]) -> Result<Val, String>` and add one `ToolSpec` to the registry:

```rust
fn tool_math_double(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Num(arg_num_or(args, 0, 0) * 2))
}
// ... in tool_specs():
ToolSpec { module: "Math", proc: "double", sig: "Math.double(n)",
           summary: "twice n", handler: tool_math_double },
// ... and once per module, in module_specs():
ModuleSpec { name: "Math", summary: "small arithmetic helpers" },
```

After rebuilding, `{{ Math.double(21) }}` renders `42`, `Meta.tools("Math")` lists it, and the
tools page (§4) picks it up automatically. Keep handlers **pure** so pages stay deterministic.

---

## 4. Live, interactive widgets

A plain hole is computed once, on the server, when the page is rendered. The `Live` module turns
a hole into a **widget the reader can drive** without leaving the page:

```
{{ Live.box("SCArs.evolve(word)",  [["word", "kasa"]]) }}      text output
{{ Live.view("Viz.chart(kind, nums)",                          raw HTML/SVG output
     [["kind", "bars", "bars", "line", "scatter"],
      ["nums", "3 1 4 1 5 9"]]) }}
```

The first argument is any Facet expression; the field names in it become editable controls. The
field list is `[[name, default, …], …]`, and the extra items pick the control:

| Field shape | Control |
|-------------|---------|
| `[name, default]` | a text input |
| `[name, default, opt1, opt2, …]` | a `<select>` dropdown of the options |
| `[name, default, "~", min, max, step]` | a range slider |

`Live.form` is the ACTION variant: the same inputs plus a **Go** button, and the expression
runs only when the button is clicked — never at render time (viewing a page must not perform
the action), never on keystrokes, and never from the client cache (a second click re-executes).
It exists for side-effecting tools: `Db.post`, `Db.sync`. After a form action fires, every
other live widget on the page re-runs with its cache cleared (the `facet-action` event), so a
board you just posted to updates in place.

The `Db.*` tools surface the PERSISTENT database to pages: `Db.post(board, author, text)`
appends an entry under a Lamport-pair key, `Db.board(board, n)` renders the newest n, and
`Db.sync(board, url)` reconciles the table with a peer node (see "Shared state over the
network" in the-system.md).

`Live.watch(expr, fields, secs)` is the third sibling: a `Live.view` that re-runs itself every
`secs` seconds (clamped 1–60) with no user action — made for state that changes *behind the
page's back*: gossiped ledger events, a worker coming alive, a peer link appearing. Its field
list may be empty (a pure display). The `Kv.*` tools surface the GUI's **ledger** — the
persistent, gossiped key-value node behind `/network` — and `Net.*` the distributed-execution
layer (workers, distribution-aware eval, FedAvg training, persisted models); both are marked
volatile, so their results are never served from the render memo, and the ledger's generation
stamp keys that memo, so a gossiped event from a peer invalidates exactly the pages that show
ledger state (see docs/network-gui.md).

`Live.box` shows the result as text (escaped); `Live.view` inserts it as raw markup, so chart
and HTML tools display live. As the reader edits a control, the widget re-evaluates the
expression on the server through `POST /api/eval` and swaps in the new result — debounced, with
a small cache on both client and server, and out-of-order responses dropped. With JavaScript
off, each widget still shows its server-rendered initial value, so the page degrades cleanly.

The hosted page `lib/site/tools.facet` is built entirely from these widgets: a live frame for
every module above, so the whole tool surface is explorable in the browser.

---

## 5. Rendering pipeline

- **Hosted files.** Hymn serves `*.facet` files from the site root, rendering them per request.
  `lib/site/index.facet` is the example page; `lib/site/tools.facet` is the interactive tool
  catalogue (§4).
- **Live preview API.** `POST /api/render` with a Facet body returns the rendered HTML; this is
  what the WYSIWYG editor at `/editor` uses for its live pane.
- **Live-widget API.** `POST /api/eval` with `expr=…&name=value&…` evaluates a single expression
  with the given field values and returns its rendered result; this is what `Live.box` /
  `Live.view` widgets call as the reader edits a control. Results are cached (keyed by the
  library generation, so they refresh when a lib changes) and the parse of each expression is
  cached independently.
- **Editing.** The `/editor` page opens and saves the hosted `*.facet` files (`GET`/`POST
  /api/file?path=NAME.facet`) and is Unicode-safe, so Heart Speech glyphs like `ɣ`, `ā`, `ō̃`
  round-trip through edit → save → render byte-for-byte.

---

## 6. A complete page

```html
<!doctype html>
<h1>{{ Txt.cap("heart speech") }}</h1>
<table border="1" cellpadding="6">
  <tr><th>Solar</th><th>Heart</th></tr>
{{each w in ["ligā", "nīvō", "mazdā", "bendā", "genton"]}}
  <tr><td>{{ w }}</td><td>{{ SCArs.evolve(w) }}</td></tr>
{{end}}
</table>
{{if SCArs.evolve("kasa")}}
  <p>Derived live by SCArs on the Loom.</p>
{{end}}
```

Each row's second cell is computed by running the sound-change engine; reload after editing and
the table changes — no compilation, no client-side code.

---

## 7. Gotchas

- **Strings use double quotes.** `"zelā"`, not `'zelā'`. Inside them the usual escapes work
  (`\n`, `\t`, `\"`, `\\`), which matters when an expression is itself a string argument to
  `Live.box`/`Live.view`.
- **List items are comma-separated:** `["a", "b"]` (Facet), unlike Latte's space-separated
  `[a b 0]`.
- **`if` truthiness is "non-empty is true"** — the opposite of Latte's loobean. An empty
  string / `0` / empty list is false.
- **Expressions are pure.** There are no loops or assignment; iteration is `{{each}}`, and the
  only binding form is `let … in …`. Computation belongs in the tools (Latte), not the page.
- **Unknown tool → error.** `Module.proc` that isn't in the dispatch table renders an error
  string; add it in `src/facet.rs`.
