# Orpheus update — the network, in the browser: the ledger, the /network page, and one tool registry

## The GUI hosts a node now

`latte gui` starts a **ledger**: a persistent, gossiped key-value node — the
same kv agent, wire protocol, and durable store `latte node` runs, hosted
inside the GUI server (listening on `:9600` by default; `--kv-store DIR` for
durability, `--kv-peer ADDR` to dial peers at startup, `--kv-listen off` to
only dial out). A GUI instance and a bare CLI node are full peers of one
another, because they are the same machinery.

## /network — connecting instances is a form, not a command line

The new page (header: **⇄ Network**) is the interactive interface to the
network layers. **Connect a peer** dials another instance's ledger with a
retry-forever connector added at *runtime* (`net::connect_peer`, new) — you
can connect first and start the peer later, and one direction suffices.
**The shared ledger** panel puts durable, gossiped keys; peers fold events in
one agreed total order, so every connected instance's page converges on
byte-identical state within seconds. **History & time travel** puts the
event-sourced past on a slider (`Kv.at(k)`) with the agreed event order
alongside (`Kv.log`). **Workers** manages the distribution registry, runs
distribution-aware evaluations (`Net.eval`), and **Distributed training →
persistent model** runs the whole FedAvg loop from the page: cycles of
distributed execution and consolidation, the final consolidated model
committed as one event in a named store, then read back and *used* —
`Net.predict(store, x)` computes from the persisted model. Ledger values are
stored self-describing (`[%n 42]` / `[%t "…"]`), so what one instance types
is exactly what every peer displays — and raw values from CLI nodes still
render sensibly.

## Facet grew a widget that follows the network

`Live.watch(expr, fields, secs)` re-runs its expression every few seconds —
made for state that changes behind the page's back: a gossiped event landing,
a worker coming alive, a peer link appearing. Its field list may be empty.
Underneath, the ledger's generation stamp now keys Facet's render and
expression memos, so a peer's event invalidates exactly the pages that show
ledger state and nothing else; `Kv.*`/`Net.*` results are marked volatile
(live peer-link and worker-liveness facts) and are never served stale from
the memo.

## One command namespace, three surfaces

The System console's `Module.proc` commands now dispatch through the same
tool registry Facet pages use (`facet::run_host_tool`): `Kv.put greeting
"hello world"` (quotes group arguments), `Kv.state`, `Net.workers`,
`Net.train 3 300 demo` — typed into any text, wired into any page widget, or
driven from the CLI, they are the same handlers. Tables flatten to console
text automatically. Model stores are *named*, not pathed — pages never touch
the filesystem directly.

Tests: runtime peer connection reconciling two live TCP nodes with traffic
both before and after the link; ledger put/state/time-travel/log round-trips;
`Live.watch` markup, clamping, and runtime polling; console dispatch through
the shared registry with quote-aware arguments — plus the registry-driven
tools-page completeness test now covering every `Kv.*` and `Net.*` tool.
Docs: `docs/network-gui.md`, and updates to `docs/facet-language.md` and the
README.
