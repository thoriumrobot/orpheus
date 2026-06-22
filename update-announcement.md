# Orpheus update — the interactive tool surface

This release turns the Orpheus environment's tools from something you call *inside* a page into
something you can **drive in the browser**. The headline is a new hosted page — open the GUI and
visit **`/tools`** — where every tool the system exposes is a live, interactive widget. Type in a
field, drag a slider, pick from a dropdown, and the result updates as you go. The home page now
links to it.

Nothing about the environment's philosophy changed: pages are still pure Facet markup, still
rendered on the server, still a deterministic function of their tool outputs. What's new is that
a single page can now let a reader *explore* that surface end to end.

## Live widgets

Two new Facet tools, `Live.box` and `Live.view`, wrap any Facet expression in a self-updating
control:

```
{{ Live.box("SCArs.evolve(word)", [["word", "kasa"]]) }}
{{ Live.view("Mkt.chart(kind, n)",
     [["kind", "line", "line", "bars", "scatter"], ["n", "60", "~", "2", "1318", "1"]]) }}
```

The field names in the expression become editable controls — a text input, a `<select>`
dropdown, or a range slider, chosen by the field's shape. `Live.box` shows the result as text;
`Live.view` inserts it as raw markup, so chart and HTML tools render live. As you edit, the
widget re-evaluates the expression on the server through a new `POST /api/eval` endpoint and
swaps in the result — debounced, cached on both client and server, with stale out-of-order
responses dropped. With JavaScript off, every widget still shows its server-rendered initial
value, so the page degrades cleanly and stays byte-for-byte deterministic per request.

## A bigger, self-describing tool catalogue

The tool registry now spans **11 modules and 44 tools**, all reachable from `/tools`:

- **`SCArs`** — the sound-change applier: PIE → Solar, Solar → Heart, plus an arbitrary-rule
  engine (`apply`) and a step-by-step `trace`.
- **`Txt`** — eleven composable text helpers (case, reverse, trim, words, split/join, replace,
  escape, length).
- **`Latte`** — the whole Latte language, evaluated live from a text box.
- **`Viz`** — data as SVG charts (bars, line, scatter).
- **`Mkt`** — a market-data lab over a built-in price series: query and aggregate it, chart it,
  measure realized volatility, get a momentum-vs-moving-average signal, and a next-day
  volatility **forecast from a HAR-RV model** (the same volatility model the trading advisor
  uses).
- **`Date`** — civil date arithmetic (offsets, day count, weekday, ordinal conversions).
- **`Sent`** — text sentiment scoring (polarity, label, word counts).
- **`Hash`** — SHA3-256 digests and short fingerprints.
- **`Phono`** — phonology: built-in inventories, deterministic word generation, typological
  reports, and a custom-inventory coiner.
- **`Meta`** — the environment describing *itself*: `modules()`, `tools(module)`, and `count()`
  read the very registry that drives every page, so the catalogue can never list a tool that
  isn't actually callable.
- **`Live`** — the widget builders above.

Because the registry is data, the tools page is generated against it and a test enforces that
**every** registered tool gets both documentation and a working live invocation — the page can't
silently drift from the code.

## Controls that reach the whole tool

The interactive controls now expose each tool's full input domain rather than a convenient
slice: the market sliders span the entire price series, chart length is a free slider rather
than a handful of presets, date offsets reach years in either direction (including pre-epoch
ordinals), and the phonology and hashing controls cover their full ranges. If a tool accepts it,
you can now ask for it from the page.

## A better-documented standard library

The Latte libraries that everything else builds on are now far more thoroughly commented,
explaining *how the code works*, not just what it does. `std.lat` now spells out the conventions
the whole codebase relies on — naturals built Peano-style from a single successor primitive, the
`loop … again` iteration form, the `fast %name` jet hints, and the Nock-style loobean where `0`
means true — alongside a line-by-line account of the arithmetic core. The Mocha application
pattern (`poke`/`peek` with tagged actions) is documented in full on the canonical to-do app and
referenced from the others, and `tensor.lat`'s matrix multiply is annotated end to end.

## Quality

The full test suite — 371 tests — passes, including the registry-enforcement test for the tools
page, the determinism checks that guarantee native and interpreted execution agree, and the
round-trip tests for the live-evaluation endpoint. Documentation has been updated to match:
see [`docs/facet-language.md`](docs/facet-language.md) for the tool registry and live widgets.

## Trying it

Build and run as before (see [`BUILD.md`](BUILD.md) and
[`docs/building-and-running.md`](docs/building-and-running.md)), launch the GUI, open the printed
URL, and visit **`/tools`**. Or, from the home page, follow the new “interactive tools page” link.
