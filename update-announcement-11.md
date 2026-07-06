# Orpheus update — the usability round: one-stop peering, deep links, and the adaptive engine everywhere

## Connecting two instances is now ONE move

`Net.connect <host>` dials the peer's ledger (host:9600) AND its shared
documents (host:9601) together — the /network page leads with the form, the
console takes the same command, and both links retry forever and persist.
The old per-layer forms remain for the unusual cases; the startup notice
covers both node ports in one line.

## The editor learned the web's manners

/notes documents are **deep-linkable** — opening a document puts
`?id=nLAM-HEX` in the address bar, so sharing a document is sharing a URL,
and the link opens straight into it. A live **peer pulse** in the toolbar
shows how many gossip links the notes node holds right now. And documents
can finally be **removed** (🗑, with confirmation) — `Note.remove` drops the
document from the live view while the log keeps its whole past, so the
history slider still replays it.

## The adaptive engine, everywhere an expression runs

`Latte.eval` (every live widget on the tools page) and `Code.run` (shared
code documents) now evaluate through the ADAPTIVE engine instead of the bare
interpreter: page evaluations profile themselves, compile when measured hot,
and distribute eligible shapes across registered workers — the same policy
`latte eval` applies, so `latte profile --list` now reflects what the pages
actually compute. One evaluation policy, every surface.

## Discovery

The System console's boot text gained a WORK TOGETHER section pointing at
the Network and Notes surfaces and naming the commands; the front page
mentions the shared-documents editor; and the README opens with "Two
machines, five commands" — gui, gui, Net.connect, write together, lend a
worker.

Tests: one-stop connect address derivation (explicit and default ports, with
the retry loops cancelled after), document removal with history preserved,
and the existing registry-coverage test now enforcing interfaces for
`Net.connect` and `Note.remove`. The tools-page test also pins the adaptive
`Latte.eval` to its exact classic output — same values, better engine.

## The audit round, part two

A fresh pass over the new surfaces found four issues, all fixed and tested.
**`Kv.at` could strand the slider in the past**: it indexed history from the
beginning, so a fixed 0–200 slider could never reach the present once the
log outgrew it — the tool now counts BACK from now (0 = present), so any
slider always reaches the latest state and the recent past.
**`Net.connect ::1` mangled IPv6**: multi-colon inputs are now refused with
directions to the explicit per-layer forms, and the adjacent-port assumption
behind the one-stop derivation is documented. **The editor gossiped a
retitle event on every title blur** — now only a changed title becomes an
event. And **typing into a block a peer deleted silently discarded the
words** (the set landed on a tombstone): the editor now says so and shows
your text so nothing is lost silently. One suspected issue proved already
handled: repeated `Code.run` does not churn the library generation, because
runtime registration skips identical source.
