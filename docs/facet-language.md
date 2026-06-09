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

## 3. Built-in tools

Tools live in the host's dispatch table (`dispatch_tool` in `src/facet.rs`). The ones shipped:

| Call | Result |
|------|--------|
| `SCArs.evolve(word)` | evolve a Solar word into Heart Speech through the full Ligurian pipeline (see [`scars-sound-changes.md`](scars-sound-changes.md)) |
| `SCArs.apply(word, rule1, rule2, …)` | apply *arbitrary* sound-change rules to a word (the general engine) |
| `Txt.upper(s)` | uppercase |
| `Txt.cap(s)` | capitalise the first letter |
| `Txt.join(list, sep)` | join a list into text with a separator |

The sound-change tool runs on the Loom via the Latte engine (`lib/sca.lat`), so a Facet page
is genuinely computing its content through the same VM as everything else.

### Adding a tool
A new tool is one match arm in `dispatch_tool(module, proc, args)`:

```rust
("Math", "double") => {
    let n: u128 = arg_text(args, 0)?.parse().unwrap_or(0);
    Ok(Val::Text((n * 2).to_string()))
}
```

After rebuilding, `{{ Math.double(21) }}` renders `42`. Keep tools pure so pages stay
deterministic.

---

## 4. Rendering pipeline

- **Hosted files.** Hymn serves `*.facet` files from the site root, rendering them per request.
  `lib/site/index.facet` is the example page.
- **Live preview API.** `POST /api/render` with a Facet body returns the rendered HTML; this is
  what the WYSIWYG editor at `/editor` uses for its live pane.
- **Editing.** The `/editor` page opens and saves the hosted `*.facet` files (`GET`/`POST
  /api/file?path=NAME.facet`) and is Unicode-safe, so Heart Speech glyphs like `ɣ`, `ā`, `ō̃`
  round-trip through edit → save → render byte-for-byte.

---

## 5. A complete page

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

## 6. Gotchas

- **Strings use double quotes.** `"zelā"`, not `'zelā'`.
- **List items are comma-separated:** `["a", "b"]` (Facet), unlike Latte's space-separated
  `[a b 0]`.
- **`if` truthiness is "non-empty is true"** — the opposite of Latte's loobean. An empty
  string / `0` / empty list is false.
- **Expressions are pure.** There are no loops or assignment; iteration is `{{each}}`, and the
  only binding form is `let … in …`. Computation belongs in the tools (Latte), not the page.
- **Unknown tool → error.** `Module.proc` that isn't in the dispatch table renders an error
  string; add it in `src/facet.rs`.
