# Orpheus update — one engine per system, every interface live, and state shared across the network

## One advisor, every surface

The trading advisor had grown three bodies: a CLI printer for price markets, a separate
printer for the bond desk, and an HTML renderer behind `/api/trade`. The MODEL was
always singular; now the report is too: `advice_text(market, …)` routes Treasuries to
the duration model and any price tape to the TA/volatility/news engine, and returns the
full report — the CLI prints it, the new `Trade.advice` page tool serves it, and the
two can never drift because they are the same function.

## Switch markets from the advisor — everywhere

The `/trade` page gained a market selector fed by `/api/markets` (one curated list —
`marketdata::MARKETS` — serving the page, the tools panel, and the docs); changing it
re-runs the advisor live. The `/tools` page carries the same switch as a dropdown on
the `Trade.advice` widget: bonds, BTC, ETH, and the rest, one click apart.

## Facet grew a verb: `Live.form`

Live widgets could observe; now they can ACT. `Live.form(expr, fields)` renders inputs
plus a Go button, and the expression runs only on the click — never at render time
(viewing a page must not perform the action), never on keystrokes, never from the
client cache (a second click re-executes). When a form fires, every other live widget
on the page re-runs with caches cleared, so what you changed updates in place.

## The Board: persistent, multi-user, networked — through the GUI

`/board` is a message board backed by the write-ahead-logged database: posts survive
restarts, every browser and every user of a node shares them, and one user's post is in
every other user's very next render — the page memo now keys on the DATA generation, so
shared pages are never stale and never slow (the same cache discipline code changes
already had).

And boards span SYSTEMS. Posts carry Lamport-pair keys — the time, then the node's
persistent id as tie-break, the exact total order of `lib/lamport.lat` (pinned by a
differential test against the library itself). Keys are unique and records immutable,
so two nodes' boards merge as a G-Set from the CRDT playbook (`lib/crdt.lat`):
`Db.sync(board, url)` on the page, or `latte db sync http://host:8088 board` from a
shell, reconciles over the ordinary `/api/db` endpoints — pull what the peer has, push
what it lacks — idempotent, order-independent, convergent. Verified live: two nodes
posting independently, one sync, both boards agree; a resync is a no-op; a third
system joins with one command; a restart replays the WAL and nothing is lost. Records
travel as re-evaluable Latte expressions (the checkpoint's own serializer), so they
survive the trip byte-exactly; genuine conflicts are kept local and reported, never
silently clobbered.

## The reuse ledger

This release added no new engine it could borrow: storage is the existing WAL database
(now with `keys`, `rec`, `field_text`, and a readable data generation); the wire is the
zero-dependency HTTP client the binary registry already used (now with a POST half);
ordering is `lib/lamport.lat`'s; merge semantics are `lib/crdt.lat`'s; the widgets are
the one Live machinery; the advisor is the one advisor. The new `db.lock`-style pieces
are glue, and the glue is tested.
