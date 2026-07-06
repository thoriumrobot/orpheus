# The Network page — connecting Orpheus instances through the GUI

`latte gui` now hosts a **ledger**: a persistent, gossiped key-value node on
the same layer as `latte node`. The `/network` page (linked from the System
console header as **⇄ Network**) is the interactive interface to it — plus
the distributed-execution layer — so connecting instances over a LAN or the
open Internet, and making productive use of the shared persistent state, is
done entirely from the browser.

## Starting

```sh
latte gui                                  # ledger listens on 0.0.0.0:9600 (in-memory)
latte gui --kv-store ~/ledger              # durable: events survive restarts
latte gui --kv-listen 0.0.0.0:7500         # a different port (or `off` to only dial out)
latte gui --kv-peer 203.0.113.7:9600       # dial a peer at startup
```

The GUI prints its ledger identity on startup:

```
ledger node id=18bf9b1e8e55cc5d listen=0.0.0.0:9600 events=0 store=(memory)
```

## Connecting two instances

Run `latte gui` on each machine, open `/network` on either, and paste the
other machine's address (`host:9600`) into the **Connect a peer** form. One
direction suffices — the sync protocol exchanges events both ways over a
single link. Connectors **retry forever**, so you can connect first and start
the peer later; behind NAT, forward the ledger port on one side or meet on a
host with a public address (the wire is plain TCP, length-framed jammed
nouns — architecture-independent, so a PC, a phone, and a server are full
peers).

The **This instance** panel refreshes itself every few seconds
(`Live.watch`), so an arriving link or a gossiped event shows up on its own.

## The shared ledger

**Kv.put** on the page (or `Kv.put greeting "hello"` typed into any System
console text — pages and the console share one tool registry) appends a
durable event to the log and pushes it to every connected peer. Peers fold
events in one agreed total order (Lamport time, node id as tie-break), so
connected instances converge on **byte-identical state** — no consensus
round-trips, no conflict dialogs. The state table on every connected
instance's page updates within a few seconds of any put, anywhere.

Because the log *is* the state, history is free: the **time-travel slider**
(`Kv.at(k)`) replays the shared state as of any prefix of events, and
`Kv.log(n)` shows who did what in the agreed order.

Values are stored self-describing (`[%n 42]` for numbers, `[%t "…"]` for
text), so what you type is exactly what every peer displays. Raw values
gossiped by a bare CLI node (`latte node --do "put k 5"` pointed at the
ledger port — a GUI and a CLI node are the same kind of peer) display via a
printable-cord-else-decimal fallback.

## Workers and distributed training, from the page

The **Workers** panel manages the same registry as `latte workers`: add the
address of any instance running `latte worker` (a phone in Termux counts) and
distribution becomes the default for eligible work. `Net.eval` runs a Latte
expression with distribution — a distributable shape (`dmap`, a
measured-heavy `map`, `predict_all`) splits across the registered workers,
anything else runs locally, and the result says which happened.

**Net.train(rounds, iters, store)** runs the FedAvg cycle from the page: each
round, every worker trains on its own data shard; the models are consolidated
in Latte (`fedavg`, lib/dist.lat); the full-data error falls cycle over
cycle. Naming a store commits the final consolidated model as **one** event
in a durable log under the cache directory (stores are named, not pathed —
pages never touch the filesystem directly). `Net.model(store)` reads it back,
and `Net.predict(store, x)` computes y = w·x + b from the persisted model —
the trained state, productively used from a page.

## One tool registry, three surfaces

Every widget on `/network` (and `/tools`) is a `Module.procedure` call into
one registry (`src/facet.rs tool_specs`). The System console dispatches the
same commands — `Kv.state`, `Net.workers`, `Net.train 3 300 demo`,
`Kv.put greeting "hello world"` (quotes group arguments) — with tables
flattened to console text. And the CLI drives the same machinery:
`latte node`, `latte worker`, `latte workers`, `latte ml linear --store DIR`.
A page, a console text, and a shell are three surfaces of one system.

## Live.watch — pages that follow the network

`Live.watch(expr, fields, secs)` (see docs/facet-language.md) is the third
live-widget sibling: it re-runs its expression every `secs` seconds, so state
changing *behind the page's back* — a peer's gossiped event, a worker coming
alive — appears without any user action. Its field list may be empty. The
ledger's generation stamp also keys Facet's render memo, so a gossiped event
invalidates exactly the pages that show ledger state, and nothing else.

## The trust model — read this before exposing ports

The gossip protocol (ledger/`latte node`, default `:9600`) and the worker
protocol (`latte worker`, default `:9700`) are **unauthenticated by design**:
any host that can reach the port can inject ledger events or submit
evaluation tasks. This is the trusted-peers model — right for a LAN, a
tailnet/VPN, an SSH tunnel (`ssh -L 9600:localhost:9600 host`), or machines
you own; wrong for a port forwarded to the open Internet unprotected. The
GUI's own HTTP port binds `127.0.0.1` by default, so pages and the console
are local unless you choose otherwise; `--kv-listen off` makes the ledger
dial-out only. Convergence is still safe in the Byzantine-free sense — events
are content-addressed and deduplicated, and a malformed frame drops the
connection — but *authorization* is the network's job, not the protocol's.

Two operational notes: `Kv.connect` persists (a restart redials;
`Kv.forget` undoes it), and a node that accidentally dials its own address
is detected by id and dropped.

## The pieces, and where they live

| piece | source |
|---|---|
| the ledger node (kv agent, durable log, gossip) | `src/ledger.rs` on `src/net.rs` + `src/agent.rs` |
| runtime peer connection (retry-forever) | `net::connect_peer` |
| `Kv.*` / `Net.*` page tools | `src/facet.rs` |
| the console bridge (one command namespace) | `facet::run_host_tool` ← `serve::run_tool` |
| distribution, FedAvg, model stores | `src/dist.rs` (docs/distributed-execution.md) |
| the page | `lib/site/network.facet` |
| shared-dataset training (`Net.trainLedger`) | ledger `data_points` + `dist::fedavg_linear`, auto learning rate |
