# Distributed execution — connected instances share the work

The gossip layer (`src/net.rs`) makes connected Orpheus instances agree on
**state**; the distribution layer (`src/dist.rs` + `lib/dist.lat`) makes them
share **work**. Because every Latte program is a pure function of its source,
a computation produces the same noun on any instance — so remote execution is
an *audited acceleration of a pure meaning*, the jet principle applied across
machines. Distribution can change how fast an answer arrives, never what the
answer is.

## Workers

A worker is a full Orpheus instance that answers evaluation tasks over TCP
(length-framed `TASK`/`RESULT` messages carrying jammed nouns):

```sh
# on each helper machine (or another shell):
latte worker --listen 0.0.0.0:9700

# on the coordinating instance:
latte workers add 192.168.1.20:9700
latte workers                      # list, with liveness
latte workers rm 192.168.1.20:9700 ; latte workers clear
# or per-run: ORPHEUS_WORKERS=host1:9700,host2:9700 latte eval "..."
```

Once workers are registered, **distribution is the default** — no flag, no
annotation. Opt out per run with `--no-dist`, or globally with
`ORPHEUS_DIST=0`.

## The profiler detects distributable shapes

The code profiler already measures every program the adaptive engine sees and
uses the measurement to decide compile-vs-interpret. It now also detects the
**data-parallel shapes** and applies the same measurement-beats-guesswork
policy to distribute-vs-stay-local:

- `(dmap f xs)` — the *distributable map* (`lib/dist.lat`). Its meaning is
  exactly `(map f xs)`; distributing whenever workers are connected is what
  the name says.
- `(map f xs)` — distributes once its **measured** interpreter time crosses
  the distribution threshold (`ORPHEUS_DIST_NS`, default 25 ms), below which
  the network round-trips could not pay.
- `(predict_all w b xs)` — ML batch prediction (`lib/ml.lat`): the model is
  broadcast, the inputs are sharded. Same measured-threshold policy.

`latte profile "<expr>"` reports the detection and the decision:

```
profile: (dmap (fn [x] -> (mul x x)) [ 1 [ 2 [ 3 [ 4 [ 5 [ 6 0 ] ] ] ] ] ])
  interpreter       0.031 ms (expression; scope baseline 0.011 ms subtracted)
  ...
  distributable: data-parallel map (dmap — distributes whenever workers are connected)
  dist decision: distribute by default across 2 connected worker(s) (dmap is the distributable map)
```

Execution splits the list into contiguous chunks, one task per worker
(`(map f chunk)`), and concatenates the results in order. A worker that is
unreachable or fails mid-task has its chunk **re-run locally**, so a degraded
cluster still returns the complete, correct answer. `ORPHEUS_DIST_AUDIT=1`
additionally recomputes the whole expression on the local interpreter and
compares — the distributed engine is held to the interpreter's answer,
exactly like a jet audit.

## Distributed model training — cycles of execution and consolidation

Training distributes by **local SGD with periodic model averaging** (FedAvg;
McMahan et al. 2017, arXiv:1602.05629). With workers connected,
`latte ml linear` runs distributed *by default*:

```sh
latte ml linear --store /tmp/model      # add --rounds N --local-iters E to tune
```

Per round:

1. the training data is round-robin **sharded** across the workers
   (`shard` in `lib/dist.lat` specifies the split);
2. every worker runs `train` (`lib/ml.lat`) for E local gradient steps on its
   own shard, starting from the current consolidated model;
3. the coordinator **consolidates** the returned models with
   `(fedavg models sizes)` (`lib/dist.lat`) — the shard-size-weighted model
   average Σ (nₖ/n)·wₖ, computed in Latte on Loom, not in Rust;
4. the consolidated model becomes the next round's starting point.

The report prints the full-data MSE after each consolidation — the cycles
improve the model round over round:

```
  round 1: consolidated model MSE = 0.008
  round 2: consolidated model MSE = 0.007
  round 3: consolidated model MSE = 0.006
  round 4: consolidated model MSE = 0.006

  learned w = 2.022
  learned b = 0.820
  persistent state updated: final consolidated model committed as ONE event (kv %model) in /tmp/model
```

**Only the final consolidated model touches the persistent state**: one
`%put` event on the kv agent, appended to the durable, gossiped event log
(and snapshotted). The intermediate rounds are scratch work — they never
enter the log, so the log stays small and every connected instance converges
on exactly one model update per training run.

`--dist` forces the round/consolidation cycle even with no workers (shards
then train locally — useful for trying the flow on one machine);
`--no-dist` keeps the classic single-machine gradient descent.

## The Latte library (`lib/dist.lat`)

Pure arms, linkable via `import dist` — the *specification* the Rust host is
held to:

| arm | meaning |
|---|---|
| `dmap f xs` | the distributable map — exactly `(map f xs)` |
| `shard k xs` | round-robin split into k shards |
| `unshard ss` | inverse of `shard` — interleave back |
| `sizes ss` | shard lengths (the FedAvg weights) |
| `wmean vs ws` | weighted mean of signed numbers |
| `fedavg models ws` | shard-size-weighted `[w b]` model average |

## Guarantees and failure modes

- **Same answer everywhere.** Tasks are pure; a worker computes the noun the
  local interpreter would. The audit mode verifies this end-to-end.
- **Fault tolerance.** Per-chunk local fallback; the coordinator degrades to
  ordinary local evaluation with zero workers.
- **Oversized atoms.** Task data is shipped as source literals; an atom past
  the lexer's `u128` range cannot be rendered, and the computation silently
  stays local — degradation is always to the correct, slower path.
- **Persistent state discipline.** Distributed cycles run off-log; exactly
  one event carries the final consolidated result.
