# The System — texts, viewers, and embedded objects

Orpheus's GUI (`latte serve`, then open `/`) is built on the Oberon model: the
display is **tracks of viewers**; a viewer is a title bar (name + a command
menu) over a main frame; and the system's primary medium is the **text** — not
a terminal, not a menu tree. A text is a sequence of elements, where an element
is an ordinary character **or an embedded dynamic object** (a chart, a drawing,
a render, a report). You do work by *running lines of text in place*.

## Running commands

Middle-click any command line — in any text, any tool, the Log, even output a
tool printed — and it runs. (No middle button: put the caret on the line and
press Ctrl/⌘+Enter.) Lines starting with `#` or `::` are comments.

Tools run **in parallel**: Hymn serves every request on its own thread, so you
can middle-click `trace w=160 h=120`, then `trade live=1`, then keep editing a
document while both fill in.

## Opening documents, modules, and pages

The system holds several *kinds* of thing, and each kind has its own open
command. The names look alike, so the trick is matching the command to the
kind. In particular **`System.Open` opens module source, while `System.OpenText`
opens a saved text** — reaching for `System.OpenText` to open a module is the
common slip, and it reports `no such text` (it only ever looks in `text/`).

| To open… | Command | Lives in | Opens as |
|----------|---------|----------|----------|
| a **module's source** (a library or your package) | `System.Open <name>` (or `Edit.Open <name>`) | `lib/`, `pkg/` | an editable source frame titled `<name>.Mod`, with Compile / Store / Format in its menu |
| a **new, empty module** | `System.New <name>` | — | a source frame seeded with a `core <name>` skeleton |
| a **saved text** (one you wrote and Stored) | `System.OpenText <name>` | `text/` | a text frame, embedded objects rehydrated by re-running their commands |
| a **new, blank text** | `System.NewText [name]` (or the header **✚ Text** link) | — | an empty text frame — blank, named if you pass a name |
| a **manual / docs page** | `System.Edit <name>` (or the **Docs** link / `System.Docs`) | `docs/` | a rendered, editable document frame (Syntax shows the raw markdown; Save writes back) |
| a **tool text** (Trade, Plan, Charts, Db, …) | `System.Tool <name>`, or the matching header link | built in, or `text/<name>-tool.md` | a tool text with live buttons and fields |
| a **hosted page** (Draw, Chess, Phono, Editor, Grammar, …) | `System.Page <path>`, or a Contents / header link | served by Hymn | a page viewer inside the desktop (the menu's **↗ Tab** pops it to a browser tab) |

Two rules of thumb resolve almost every "which command?" question:

- **Type the bare name, never the frame's title.** A module opened with
  `System.Open btree` is *titled* `btree.Mod`, but you open it with `btree`:
  `System.Open btree.Mod` fails, because the `.Mod` is display-only and a module
  name is letters/digits/`_` with no dot. Likewise a text Stored as `notes`
  reopens with `System.OpenText notes`, not `notes.Text`.
- **Source vs. document.** `System.Open` / `System.New` are for *code you
  Compile* (it lives in `lib/` or `pkg/`); `System.OpenText` and `System.Edit`
  are for *prose you read or run* (texts in `text/`, manuals in `docs/`).

You rarely need to remember a name. **Click to open:** the **Modules** viewer
(right track) lists every loaded module — click one to open its source; the
**Contents** viewer above it indexes every manual page, tool text, and hosted
page — click an entry to open it. **List from a command:** `System.Modules`
refreshes the module list, `System.Texts` lists the saved texts on disk, and
`System.Docs` opens the documentation index. However a frame was opened, it then
moves, resizes, marks, and Stores like any other.

## Texts with embedded objects

Inside a **text frame** (System.Tool is one; the header's **✚ Text** link or
`System.NewText` opens a fresh blank one, and `System.NewText <name>` names it
up front), an object-producing command embeds its output *into the text*,
directly under the line you ran:

```
Here is the market this quarter:
chart market days=120          <- middle-click this line
                               <- the chart appears HERE, inside your text
The reflection demo:
trace w=96 h=72                <- and the render lands here
```

This is how you **insert charts from the visualization tool into a text**: type
the chart command where you want the figure, and run it. The object remembers
its command (shown in its corner); middle-click the object to re-run it in
place (a `chart market live=1` refreshes to today's prices); select it and
press Backspace to delete it. Object-producing commands: `chart`, `gfx`,
`trace`, `ta`, `trade`, `fin`, `gpu`, `derive`, `plan`.

Run the same commands from a non-text frame (the Log, a module) and each tool
fills its own output viewer instead (`Chart.Out`, `Trade.Out`, …) — several at
once.

## Naming and saving texts

A new text starts **blank and untitled** (the header's **✚ Text** link, or
`System.NewText`). To save it, press **Store** in the text's menu: if the text
has no name yet, Store **asks for a file name**, then writes it to
`text/<name>.md` and **renames the frame to match** — so it now has a title and
the next Store goes to that same file (no more prompts). You can also name it up
front when you create it (`System.NewText <name>`), or save under a chosen name
at any time with **`Edit.StoreText <name>`** — a Save-As that both writes
`text/<name>.md` and renames the frame. File names use letters, digits, `_` and
`-` (no spaces or dots); an empty or cancelled name saves nothing.

Texts persist **with their objects**: ordinary lines save as markdown, and each
embedded object as a ` ```tool <command>` fence recording the command that made
it. Loading (`System.OpenText <name>`) replays each fence — the objects
**rehydrate by re-running their commands**, so a stored text with a live chart
reopens with a live chart. `System.Texts` lists what's on disk; texts live in
the top-level `text/` directory (alongside the technique tutorials).

Documents under `docs/` open as plain editable frames with `System.Edit <name>`
(Save writes back); open as many as you like, side by side.

## Tools are texts

The header links (Trade, Charts, Finance, Plan, Acplan, Db, Algo, Dsa, Wgraph, Numth, Bits, Strings, Grid, Design, Trees, Dp, Intervals, Search, Graphs, Backtrack, Greedy, Sym, Findb, Data, Trace,
Derive, Debug, Conlang, GPU, Net, News, Forge, Format, Docs) open **tool texts** — editable
command sheets, exactly Oberon's `Draw.Tool` idea. (Acplan drives the accountable-planning
tool — quadratic vote → plan → vouchers → audit — Db drives the composed database — Algo demonstrates the five algorithm-design paradigms (Skiena), Dsa the interview data structures & coding patterns, Wgraph the weighted-graph algorithms (Dijkstra, MST), Numth the number-theory warm-ups (GCD, sieve, modular exponentiation), Bits the bit-manipulation tricks (on bitwise ops now in std), Strings the string algorithms (reverse, anagrams, Rabin-Karp search), Grid the matrix/grid algorithms (number of islands, grid DP, spiral), Design the data-structure design classics (min-stack, queue from two stacks, LRU cache), Trees the binary-tree techniques (traversals, height, LCA, validate-BST), Dp the dynamic-programming classics (knapsack, LCS, subset-sum, coin-change ways), Intervals the interval techniques (merge, meeting rooms, intersection), Search the binary-search variants (lower/upper bound, rotated arrays, peak finding, search-on-the-answer), Graphs the graph decision problems (topological sort, cycle detection, components, bipartite), Backtrack the combinatorial search problems (generate parentheses, combination sum, palindrome partitioning, word search), Greedy the greedy algorithms (jump game, gas station, partition labels, candy, task scheduler) —
state, index queries, MVCC history — Sym is a database-backed symbol index over
the system's own code: which modules define a name, with shadowing flagged —
Findb stores a window of market prices in that same database and reads it back for a
sparkline and a lag-1 model — and Data is the **persistent** database: named stores
on an on-disk write-ahead log that survive restarts (`src/dbservice.rs`, also `latte
db …` and `/api/db`). They embed live HTML returned by pure-Latte arms in
`lib/acplan.lat`, `lib/db.lat`, `lib/symbols.lat`, and `lib/findb.lat`.)

The fifteen links from **Algo** through **Greedy** are the coding-interview *technique
modules* — heavily-commented Latte libraries, each with an interactive tutorial (opened with
`System.OpenText <name>-techniques`) and a CLI demo (`latte <module> <topic>`). Their full
catalog — arms, representation conventions, and the recipe for adding your own — is in
`docs/interview-techniques.md` (open it here with `System.Edit interview-techniques`).

## Tools in pure Latte — Rust is not required

New tools are written, formatted, compiled, run, shared, and persisted without
leaving the GUI. The pieces that make this real: Latte has **string literals**
(`"text"`, with `\"` `\\` `\n` `\t` escapes) alongside `%tags`; `std` carries
the cord toolkit (`cat`, `catall`, `joincords`, `numtext`, `fixtext`,
`bytelen`, `pow`); and the arithmetic jets are **arbitrary-precision**, so
building a kilobyte of markup multiplies kilobyte-sized atoms natively instead
of crashing at the u128 boundary (the native Anvil backend declines past that
boundary and the interpreter's big jets carry on — never a silent wrap).

A tool *renders* by returning a tagged cord: `[%svg <cord>]` or
`[%html <cord>]` from any arm embeds as a live object exactly where the
command ran — the same way chart and gfx output embeds. `hello.Mod`, compiled
at boot, demonstrates the whole pattern: `hello.badge 9` (live HTML) and
`hello.spark [3 [9 [5 [12 [7 0]]]]]` (an SVG bar chart built character by
character in Latte). `ui Tool.arm` makes interactive panels. The loop is:
System.New → write → Format → Compile → run from any text → Store (pkg/) →
Forge.Share.

## Forge — team coding

The forge is a shared snippet log on the Mocha runtime: every share is a
durable, gossiped event, so the log survives restarts and converges across
linked machines. In the GUI: `Forge.As <you>` sets your author name,
`Forge.Share [name]` shares the marked source frame, `Forge.List` prints one
**runnable** `Forge.Open <name>` line per snippet (middle-click it in the Log
— the Oberon move), and `Forge.Open` lands the code in a fresh source frame
where Compile makes it live in *your* system. `Forge.By <author>`,
`Forge.Del <name>`, `Forge.Names`, `Forge.Count`, `Forge.Clear` complete the
set. The GUI's node persists under `forge/`; `latte team --as ada --name fib
--share "<code>"` (with `--listen`/`--peer` to gossip, `--get`, `--names`) is
the CLI bridge.

## Format — the Latte formatter

To format a source frame, **mark it** — click its title bar, and a `✷` star
appears with a highlighted menu — then run `Edit.Format *` (the Format tool's
button, a line in any text, or the command line). The `*` always refers to the
marked frame, and *running a command never moves the mark*, so you can keep a
source frame marked while you drive it from the Format tool, a text, or the Log.
Each source frame also has its own **Format** button, which marks that frame and
formats it in one gesture.

`Edit.Format *` formats the marked frame canonically: structural indentation
(arms at two spaces, bodies one step in, `loop`/`case` bodies one step past their
opener with `end` aligned under it, flat let-chains), canonical spacing (tight
brackets, spaced `=`/`==`/`->`, tight `again(`), author line breaks and all
comments preserved. Formatting is **proven** meaning-preserving: the result must
re-parse to the identical AST, arm for arm, or the original is returned untouched.
The same engine is `latte fmt <file> [--write]` and `/api/fmt`.
Edit the arguments, run lines, embed outputs, Store your customized tool. The
small ↗ beside each link opens the corresponding full page when you want one.

## Modules and packages from the GUI

A module frame (`hello.Mod` on the default desktop, `System.Open <name>`,
`System.New <name>`) has Compile, Store, and Format in its menu:

- **Compile** loads the module into the *running system* immediately — the next
  command line can call it. A **package is nothing but a compiled Latte
  source**: its `core NAME` is its name.
- **Store** persists it: system libraries (names shipped in `lib/`) write back
  to `lib/<name>.lat`; everything else is a **user package** and writes to
  `pkg/<name>.lat`. Packages in `pkg/` load automatically at every startup, so
  Compile + Store = a permanent extension of the system. (`latte pkg` lists
  them; this is the whole package system — transparent by construction.)
- **Format** runs the conservative, compile-checked source formatter
  (`latte fmt` from the shell).

The Modules viewer (right track) lists every module; `·live` marks runtime-
compiled ones. Click a name to open its source. The **Contents** viewer above
it indexes the whole surface: every manual page, every tool text, and every
hosted page (Draw, Soundlib, Phono, Derive, Editor, Compiler, Chess, Xiangqi,
Trade, Finance, Charts, GPU, Plan, Docs, Grammar). A page opens **inside the
system** — a viewer whose main frame is the live page — so the desktop and
its texts stay put; the viewer's menu has **Reload**, **↗ Tab** (a browser
tab when a full window is wanted), Grow, and Close. By command:
`System.Page /phono` opens any path as a page viewer, `System.Pop *` sends
the marked page to a tab, `System.Reload *` refreshes it. Page viewers drag,
move, and grow like any other viewer.

## Viewer handling — the Oberon gestures

The gestures are Project Oberon's, carried over as directly as a browser
allows. Wirth's *How to use the Oberon System* (2015) specifies them: "A
viewer is enlarged or shrunk by clicking the left button, while the cursor is
in the title bar, and then dragging the bar up or down. A viewer is moved to
another location by also inter-clicking with the middle button." And of Grow:
"System.grow … generates a copy extending over the entire column (or over the
entire display) … we may imagine that grow lifts a viewer to an overlay in the
third dimension."

Here, identically:

- **Mark** — click a viewer's **title bar** to mark it: a `✷` star appears and
  the menu bar highlights. The mark is Oberon's star pointer — it designates the
  viewer that `*` refers to (`Edit.Format *`, `Compiler.Compile *`, `System.Move *`,
  `Forge.Share`, …). It is deliberate and sticky: typing in a body, opening a
  document, or running a command line does **not** move it, so you can mark one
  frame and operate on it from another (e.g. mark a source frame, then Format it
  from the Format tool). A command's own menu buttons mark their viewer first, so
  a frame's Format/Compile/Store buttons always act on that frame.
- **Resize** — left-drag a viewer's title bar up or down: the bar is the
  viewer's top edge, and the viewer above gives or takes the space. (The
  separator bars still work too.)
- **Move** — while dragging, **inter-click the middle button**: the viewer
  lifts (it dims and shows a dashed outline) and follows the pointer; a drop
  bar shows where it will land. Release to drop it **at any position in any
  track**. Two modern equivalents lift it as well: dragging sideways into
  another track, or starting the drag with Alt held. Esc cancels.
  `System.Move *` sends the marked viewer to the next track by command.
- **Grow** — the menu's Grow cycles normal → column → whole display (siblings
  collapse to their title bars), the flex-layout equivalent of Oberon's
  overlay; a third Grow (or growing another viewer) restores.

## Tracks — changing the number of columns

In Oberon the display is a row of vertical **tracks**, standardly a wide user
track and a narrow system track, and the layout is not fixed:
`Viewers.OpenTrack` lays a new track over any contiguous sequence of existing
ones and `CloseTrack` restores what was beneath (the hierarchy is
"three-dimensional"). Here tracks sit side by side instead of overlaying:

- **`System.OpenTrack`** (or the header's ⊞ Track) adds a column beside the
  system track, up to five. A new column is empty — a filler hint marks it
  (Oberon's filler viewer) — and is a drop target for moved viewers.
- **`System.CloseTrack`** (header ⊟) removes a column — an empty extra one if
  any, else the marked viewer's; `System.CloseTrack *` names it explicitly.
  Its viewers merge into the left neighbour. The standard two tracks stay.
- New viewers open in the **least-crowded** user column — Oberon's
  `Viewers` module likewise "delivers hints as to where it might best be
  placed". Tool outputs, sources, and texts spread across your columns
  instead of piling into one.
- **`System.Def`** (or the header's ≡ Def) looks up the definition of a
  function. **You mark the function by selecting its name, then run Def.** In a
  **code text frame** — a `.Mod` source, or any module opened with `System.Open`
  — the quickest gesture is to **double-click the function name** (that selects
  the whole word), then click **≡ Def** in the header; you can also drag the
  mouse across the name to highlight it. The same works in a text, in the Log,
  or in a doc. (As a shortcut in a code frame you may instead just click *inside*
  the name, leaving the caret in it without selecting, **provided that frame is
  the marked viewer** — click its title bar to mark it, shown by the ✷ star —
  and Def takes the word under the caret.) To skip selecting altogether, type
  `System.Def <name>` on any line and middle-click it. Either way the
  definition — the function's doc comment and body — is fetched and printed to
  the Log. When several libraries define the same name (common in the one flat
  scope) it reports *all* of them and which one wins by load order, then shows
  the winner's definition, so a name resolving to an unexpected body is no longer
  a mystery. The extractor is a Latte program (`lib/lookup.lat`); the **Sym**
  tool makes the same who-defines-what query interactive, backed by the database
  (`lib/symbols.lat`); see `data-intensive.md`.

## The desktop

Two tracks by default (more with System.OpenTrack). The left track holds your
texts, tools, documents, and modules; the right holds Contents, the System.Log
(the command trail — middle-click any line to re-run it) and the Modules
index. `System.Sentiment <text>` scores text in place with the trained
classifier; `Doc.Score <name>` scores a whole document into its evidence
table.

## Controls in texts — buttons and fields

Texts can embed live CONTROLS, declared in plain markdown (the ETH Oberon for
Windows idea: panels are documents):

- `[Label](run: command)` renders as an **embedded button**; clicking it runs
  the command, and the output embeds under the button's line like any other
  object.
- `[field: name=value]` renders as an **embedded input**; button commands
  reference fields anywhere in the same text as `$name` — so
  `account [field: account=10000]` followed by
  `[Advise](run: trade account=$account)` is a working trading panel in two
  lines of text.

Every shipped tool text is built this way — open Trade.Tool and click — and
Store serializes the controls back to the same syntax, so your panels persist
and stay editable as ordinary text.

## Documents render formatted

Texts — including the manual pages — display RENDERED by default: headings,
**bold**, *italic*, `code`, bullets, quotes, fenced code blocks, and tables
(the last two shown verbatim in monospace, so saving never reflows them).
`System.Edit <doc>` opens the formatted view; the **Syntax** menu entry opens
the raw markdown beside it (Apply re-renders), and **Save** writes the
document back. System.Tool's DOCUMENTATION section is the live index: one
button per manual page, Oberon-style.

## Creating a tool — wholly inside the GUI

A tool is a TEXT plus, when it needs new verbs, a PACKAGE. The whole loop runs
in the System; nothing requires a shell:

1. `System.New mytool` opens a module frame. Write arms in Latte (`Format` in
   the menu keeps it tidy; `debug (myarm 3)` traces it when it misbehaves).
2. **Compile** (menu): the module joins the running system immediately.
   **Store**: it persists to `pkg/mytool.lat` and loads at every startup.
3. `System.NewText mytool` opens a text. Write command lines — `mytool.myarm
   42`, `chart …`, `drawing logo`, anything — with prose between them.
   Middle-click a line: **every command's output embeds in the text**,
   including your own package's arms (a `<pre>` object for textual results,
   live SVG for charts and drawings).
4. **Store** (menu): the text persists to `text/mytool-tool.md` with its
   objects serialized as ```` ```tool ```` fences.

Stored texts survive restarts — `System.OpenText mytool` brings the tool back
with every object rehydrated — and **stored tool texts override the built-in
ones**: edit Trade.Tool to your taste, Store it, and your version is what the
Trade header link opens next session (delete `text/trade-tool.md` to restore
the default). System.Tool itself works the same way.

## The toolbox

Beyond the core texts: **✎ Draw** (header) is a full vector graphics editor
built for covers and posters as much as diagrams: canvas presets (A4 poster,
book cover, square, banner), background color, shapes, stars, pen, text with
font/size/weight control, snap-to-grid, center/middle alignment, move/resize,
layering, duplicate, undo; Store a drawing and embed it in any text with
`drawing <name>` (one tool calling another).
**Debug** opens the Loom call tracer: `debug (fib 6)` records every arm call
with its arguments and result as an expandable tree (click to step in;
`debug break=ARM …` is the breakpoint; `latte debug` is the CLI form).
**Conlang** is the linguistics suite — the sound-change library (`soundlib
grimm1 verner words=pater`), the phonology builder (`phono preset=pie n=12
changes=grimm1,verner`), and their full pages at /soundlib and /phono; the
builder calls the word generator calls the change library calls SCArs.
**Xiangqi** (/xiangqi) plays Chinese chess against the trained model — piece
values learned by gradient descent in lib/xiangqiml.lat, driving a native
4-ply search verified against the Latte rules. And the ray tracer's scene is
Latte data: `trace w=96 h=72 scene=[ … ]` renders any sphere list (the format
is documented in Trace.Tool), or edit the `spheres` arm of lib/trace.lat and
Compile.

## Command reference

A compact index of the System commands. They are all just text — middle-click a
line to run it, or click the matching header link or menu button. A trailing
`*` makes a command act on the **marked** viewer (click a title bar to mark it,
shown by a ✷ star); a name argument is the bare name, never the `.Mod`/`.Text`
title.

Open and create:

- `System.Open <module>` / `Edit.Open <module>` — open module source (`lib/`, `pkg/`)
- `System.New <module>` — new module frame (a `core` skeleton)
- `System.OpenText <name>` — open a saved text (`text/`); `System.Texts` lists them
- `System.NewText [name]` — new blank text (also the header **✚ Text** link)
- `System.Edit <doc>` — open a manual page (`docs/`); **Docs** / `System.Docs` is the index
- `System.Tool <name>` — open a tool text (or use its header link)
- `System.Page <path>` — open a hosted page in a viewer; **↗ Tab** sends it to a browser tab
- `System.Modules` — refresh the Modules list

Edit and persist (act on the marked frame):

- **Compile** (`Compiler.Compile *`) — load the marked module into the running system
- **Store** — persist: a module to `lib/`/`pkg/`; a text with `Edit.StoreText [name]` to `text/` (prompts for a name if untitled); a doc with `Edit.Save` to `docs/`
- **Format** (`Edit.Format *`) — run the compile-checked source formatter
- `System.Def` (**≡ Def**) — look up the definition of a selected function (double-click a name, then run)

Viewers and layout:

- **mark** — click a title bar (the ✷ star); `*` then targets that viewer
- `System.Grow *` / `System.Copy *` / `System.Close *` — enlarge / clone / close
- `System.Move *` — send the marked viewer to the next track
- drag a title bar to resize; middle-inter-click while dragging to relocate it
- `System.OpenTrack` (⊞) / `System.CloseTrack` (⊟) — add / remove a column
- `System.Clear` — clear the Log; `System.Quit` — stop Orpheus
