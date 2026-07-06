# Orpheus update — collaborative notes: shared documents on the event log

## Write together, converge byte-for-byte

`latte gui` now hosts a second gossiped node beside the ledger: the **notes
node** (port 9601 by default; `--notes-store DIR` for durability,
`--notes-peer` to dial at startup). Open **/notes** on two connected
instances and write the same document. Every operation — make a note,
insert a paragraph, rewrite one, delete one — is a durable event, gossiped
to every peer and folded in the agreed total order by a **pure Latte agent**
(`lib/notes.lat`), so connected instances agree byte-for-byte; the whole
history of the shared document sits on a slider, straight from the log.

## A document model that preserves intention

Convergence alone is not collaboration — naive last-write-wins per note
would let one person's afternoon erase another's. The notes agent models a
document as a **sequence of blocks with global identities**: every block
carries a `[lamport node]` id minted by its creator, so edits name their
target absolutely. On top of the log's total order, three choices make
concurrent editing behave the way people expect. Edits to *different*
blocks never conflict — both land. Insertion is **anchored** ("after block
A"), and each writer's next block chains off their previous one, so two
people typing runs of paragraphs concurrently produce two contiguous runs,
not an interleaved shuffle — a property verified by a test that races two
live TCP nodes mid-edit. Deletion is a **tombstone**: a dead block leaves
the page but keeps holding its position, so "insert after X" survives X's
concurrent deletion in place instead of falling to the end. Same-block
concurrent rewrites resolve deterministically to the later event — and the
earlier version is one slider-step away.

## An editor that stays out of your way

`lib/site/notes.html` is a thin client over the tool registry: blocks are
contenteditable divs keyed by their global ids; a poll merges REMOTE changes
into the page by id — every block except the one you are editing, so a
peer's typing appears live (briefly highlighted as it lands) without ever
stomping your caret. Enter commits and opens a block after the current one;
Backspace on an empty block tombstones it; hovering shows authorship and
id; **⇄ Connect peer** dials another instance (retry-forever, persistent,
`Note.forget` to undo); **⌛ history** scrubs the shared past. Works on a
phone — the notes node and editor ride the Android build like everything
else.

## One registry, as always

Fifteen `Note.*` tools carry the whole surface — `create`, `read`, `add`,
`after`, `set`, `del`, `retitle`, `history`, `list`, `info`,
`connect`/`forget`, and the machine forms (`index`, `blocks`) the editor
polls — usable identically from Facet pages (the /tools frame), the System
console (`Note.add nID "ada" "first point"`), and the editor. The notes
generation stamp keys Facet's render memo like the ledger's, and `latte
node --agent notes` runs the same agent from the bare CLI — a GUI editor
and a headless node are full peers.

Tests: the agent's fold (anchors, LWW, tombstones-as-anchors, time travel,
list/retitle, bid round-trips) and the two-node concurrent-edit race
(convergence, no lost blocks, contiguous runs, tombstone-anchored insert
surviving concurrent deletion). Docs: `docs/collaborative-notes.md`, plus
README and network-page pointers.
