# Collaborative notes — writing together over the Internet

Open **/notes** on any `latte gui` instance (header: **✍ Notes**). Create a
note, type, and point two instances at each other — both editors converge on
one document, with every keystroke's history kept.

## Using it

```sh
latte gui                                    # notes node listens on 0.0.0.0:9601
latte gui --notes-store ~/notes              # durable: documents survive restarts
latte gui --notes-peer 203.0.113.7:9601      # dial a peer at startup
```

In the editor: pick your name, **＋ Create** a note (or open one from the
list — the list is the converged view of every connected instance), and
write. **Enter** commits the block and opens a new one after it;
**Backspace** on an empty block deletes it; clicking away commits. Hovering
a block shows its author and id. **⇄ Connect peer…** dials another
instance's notes port (`host:9601`); the connection retries forever, works
in either direction, and persists across restarts (`Note.forget` undoes
it). **⌛ history** puts the whole shared document on a slider — every past
version, from the event log, for free.

A peer's edits land in your page within a couple of seconds, animated
briefly so you see them arrive — and they never touch the block you are
typing in (your block is merged only when you leave it).

## How the merging works

The document model is the reason concurrent editing behaves well, and it is
small enough to state completely. A note is a **sequence of blocks**
(paragraphs). Every block has a globally unique id — a `[lamport node]` pair
minted by the instance that created it, shown as `LAM-NODEHEX` — so every
edit names its target absolutely, never "the third paragraph".

Convergence itself comes from the existing event-log machinery
(`docs/network-gui.md`): every operation is a durable event, gossiped to
every peer, and folded in one agreed total order by a **pure Latte agent**
(`lib/notes.lat`) — so connected instances agree byte-for-byte, always. On
top of that total order, three modelling choices preserve *intention*:

- **Different blocks never conflict.** `Note.set` rewrites exactly the block
  it names; two people editing different paragraphs both land untouched.
- **Same block, concurrent rewrites**: the later event in the total order
  wins that block — a deterministic, block-granular resolution, and the
  loser is one slider-step away in history.
- **Insertion is anchored** (`Note.after` says "after block A"), and each
  writer's next block chains off their previous one — so two people typing
  runs of paragraphs concurrently produce two contiguous runs, not an
  interleaved shuffle. This is verified by a test that races two live TCP
  nodes.
- **Deletion is a tombstone**: a deleted block leaves the page but keeps
  holding its position, so "insert after X" survives X's concurrent
  deletion in place instead of falling to the end of the document.

## Three surfaces, one registry

The editor is a thin client over the same `Note.*` tools that appear on
[/tools](/tools) and work in the System console:

```
Note.create "design review"
Note.add nID "ada" "Problem statement"
Note.after nID 2-1a2b "bob" "inserted after that block"
Note.set nID 2-1a2b "bob" "revised text"
Note.del nID 2-1a2b
Note.history nID 3                      time travel
Note.connect 203.0.113.7:9601
```

(The console splits arguments quote-aware; the machine-form tools the editor
polls — `Note.index`, `Note.blocks(id, at)` — are in the registry too, so
anything can build on them.)

## The pieces

| piece | source |
|---|---|
| the agent: blocks, anchors, tombstones, LWW — pure Latte | `lib/notes.lat` |
| the host: node, block-id minting, ops, readers, peers | `src/notes.rs` |
| the tools | `src/facet.rs` (`Note.*`) |
| the editor | `lib/site/notes.html` |
| the transport, order, durability, history | `src/net.rs` (see docs/network-gui.md) |

The notes node shares the ledger's trust model: the port is unauthenticated
by design — LAN, VPN, or tunnel it (docs/network-gui.md, "The trust model").
Known limits, stated plainly: blocks are the merge granularity (two people
typing in the *same* paragraph concurrently keep the later version, not a
character-merge); the console surface's quote grammar has no escape, so the
editor transmits `"` as `″`; and removed notes (`Note.rmnote` has no tool —
deliberately) would need the same tombstone treatment blocks get.
