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

## Texts with embedded objects

Inside a **text frame** (System.Tool is one; `System.NewText notes` makes a
fresh one), an object-producing command embeds its output *into the text*,
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

## Saving and loading texts

Texts persist **with their objects**. The Store entry in a text's menu (or
`Edit.StoreText`) serializes the text to `text/<name>.md`: ordinary lines as
markdown, each embedded object as a ` ```tool <command>` fence recording the
command that made it. Loading (`System.OpenText <name>`) replays each fence —
the objects **rehydrate by re-running their commands**, so a stored text with a
live chart reopens with a live chart. `System.Texts` lists what's on disk.

Documents under `docs/` open as plain editable frames with `System.Edit <name>`
(Save writes back); open as many as you like, side by side.

## Tools are texts

The header links (Trade, Charts, Finance, Plan, Trace, Derive, GPU, Docs) open
**tool texts** — editable command sheets, exactly Oberon's `Draw.Tool` idea.
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
compiled ones. Click a name to open its source.

## The desktop

Two tracks (drag the bars between viewers and between tracks to resize;
Grow/Close in each menu). The left track holds your texts, tools, documents,
and modules; the right holds the System.Log (the command trail — middle-click
any line to re-run it) and the Modules index. `System.Sentiment <text>` scores
text in place with the trained classifier; `Doc.Score <name>` scores a whole
document into its evidence table.

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
