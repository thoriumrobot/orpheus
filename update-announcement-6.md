# Orpheus update — connected instances now share the work, not just the state

## Workers: evaluation as a network service

The gossip layer has always let connected instances converge on one state.
This update adds the other half: **distributed execution**. `latte worker
--listen 0.0.0.0:9700` turns any instance into a worker that answers
evaluation tasks over TCP (length-framed, jammed nouns — the same wire
discipline as gossip, but request/response). Register workers on the
coordinating instance with `latte workers add HOST:PORT` (or
`ORPHEUS_WORKERS=…`), and distribution becomes the **default** — no flag, no
annotation, `--no-dist` / `ORPHEUS_DIST=0` to opt out. Latte programs are
pure functions of their source, so a task computes the same noun on any
machine: remote execution is an *audited acceleration of a pure meaning* —
the jet principle, applied across the network.

## The profiler now detects what can be distributed

The code profiler already measures every program the adaptive engine runs and
lets the measurement (not the AST guess) decide compile-vs-interpret. It now
detects the **data-parallel shapes** too — `(dmap f xs)`, `(map f xs)`,
`(predict_all w b xs)` — and drives distribute-vs-stay-local by the same
measurement-beats-guesswork policy: `dmap` (the new *distributable map*,
`lib/dist.lat`, meaning exactly `map`) distributes whenever workers exist;
a plain `map` distributes once its measured time crosses `ORPHEUS_DIST_NS`
(default 25 ms), below which network round-trips cannot pay. `latte profile`
prints the detection and the decision alongside the engine measurements. The
list is split into contiguous chunks, one `(map f chunk)` task per worker,
results concatenated in order; a failed worker's chunk re-runs locally, so a
degraded cluster still returns the complete, correct answer — and
`ORPHEUS_DIST_AUDIT=1` holds the whole thing to the interpreter's answer,
exactly like a jet audit.

## Distributed training: cycles of execution and consolidation

`latte ml linear` now trains **distributed by default** when workers are
connected, by local SGD with periodic model averaging (FedAvg — McMahan et
al. 2017). Each round: the data is round-robin sharded (`shard`,
`lib/dist.lat`); every worker runs `train` (`lib/ml.lat`) for E local
gradient steps on its own shard from the current consolidated model; the
returned models are consolidated with `(fedavg models sizes)` — the
shard-size-weighted average Σ (nₖ/n)·wₖ, computed **in Latte on Loom**, not
in Rust — and redistributed as the next round's start. The report shows the
full-data MSE improving cycle over cycle, and then — only then — the
persistent state changes: **one** kv `%put` event carrying the final
consolidated model into the durable, gossiped log (`--store DIR`). The
intermediate rounds are scratch work; the log records the result, not the
labour. `--rounds` / `--local-iters` tune the cycle, `--dist` runs it even on
one machine, `--no-dist` keeps the classic run.

## Latte grew a distribution vocabulary

`lib/dist.lat` (`import dist`) carries the pure specification the Rust host
is held to: `dmap` (the distributable map), `shard` / `unshard` (round-robin
decomposition and its inverse), `sizes`, `wmean` (weighted mean of signed
numbers), and `fedavg` (weighted model consolidation). Everything the
distribution layer does has a meaning you can run — and test — on a single
interpreter.

Docs: `docs/distributed-execution.md`. Tests: protocol round-trips, render
round-trips, shard/unshard inverse, hand-checked `fedavg`, worker-vs-local
agreement, dead-worker fallback, and an end-to-end FedAvg run asserting
convergence, cycle-over-cycle improvement, and exactly one persisted event.
