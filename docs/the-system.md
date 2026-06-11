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
