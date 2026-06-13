# Data-Intensive Techniques — an interview guide

This manual works through the core techniques from Martin Kleppmann's *Designing
Data-Intensive Applications* (DDIA), each implemented as a small **Latte** library
in `lib/`. The goal is interview readiness: for every technique you get the idea
in one paragraph, the data structure, a runnable example, the trade-offs, and the
questions an interviewer actually asks — answered.

It closes with a tour of how **Orpheus itself** is a data-intensive system, so the
algorithms here are not toys bolted on the side: the host's event log, gossip,
logical clocks, and convergent state are the same ideas running for real.

## Running the examples

Three ways, same code:

- **Guided demo:** `latte ddia` runs every technique; `latte ddia bloom` (or
  `lsm`, `btree`, `vclock`, `crdt`, `lamport`, `chash`, `quorum`, `wire`,
  `mapred`, `merkle`) runs one.
- **Console / `eval`:** every library is in the default scope, so
  `latte eval "(bloom_test (bloom_add (bloom_make 64 3) %x) %x)"` just works.
- **GUI:** open the System page, middle-click a `… ` command line, or open a
  module from the Modules viewer (`System.Open lsm`) to read and edit the source.

A 30-second Latte refresher (full reference in `latte-language.md`): values are
**atoms** (arbitrary-precision naturals; short text is packed into an atom as a
`%cord`) and **cells** `[a b]`. Booleans are **loobean**: `0` is true, so `(a == b)`
is `0` when equal and `if (c) then A else B` takes `A` when `c` is `0`. **Lists**
end in `0`: `[1 2 3 0]`. These libraries follow one convention worth memorizing:
**a predicate/relation returns `0` for "yes."** So `(bloom_test f k)` is `0` when
`k` is possibly present, and `(vc_conflict a b)` is `0` when there is a conflict.

---

# Part I — Storage and retrieval (DDIA Ch. 3)

Every database answers two questions: where do I put a write, and how do I find it
again. Two index families dominate, and an interviewer will want you to compare
them: **B-trees** (update in place) and **LSM-trees** (append then merge). The
**Bloom filter** is the accelerator that makes LSM reads bearable.

## B-tree — `lib/btree.lat`

A B-tree keeps keys in a balanced search tree with high fan-out: each node holds
many keys and points to many children, and every leaf sits at the same depth. A
lookup walks from the root to a leaf, so it costs `O(log n)` with a small,
predictable height. Updates happen **in place** — the page holding a key is found
and overwritten — which is exactly what you want for transactional, read-heavy
workloads, and why almost every relational engine uses one.

This is the textbook (CLRS) immutable B-tree of minimum degree `t`: a node holds
between `t-1` and `2t-1` keys, and insertion splits any full child on the way down,
so a write is a single root-to-leaf pass.

```
(bt_build [[%mm 1] [[%dd 2] [[%aa 3] [[%zz 4] [[%kk 5] 0]]]]] 2)   :: build, t=2
(bt_search tree %kk)        :: -> [%val 5]   (0 if absent)
(bt_inorder tree)           :: sorted [key value] list — a full index scan
(bt_height tree)            :: every leaf is this far from the root
```

**In an interview**
- *Why is the tree always balanced?* Growth happens only at the root: when the
  root is full it splits and a new root is created above it, so all leaves rise
  together. Height changes only this way.
- *Read vs write cost?* Reads are `O(log n)` and touch one page per level. Writes
  may split pages and, on disk, often require a write-ahead log because a split
  touches several pages and a crash mid-split corrupts the tree.
- *Fan-out?* Real B-trees set node size to the disk/page block size (e.g. 4–16 KB)
  so one node = one I/O; fan-out of several hundred means a billion keys fit in
  ~4 levels.

## LSM-tree / SSTable — `lib/lsm.lat`

An LSM-tree optimizes the opposite way: never seek to update, only append. Writes
land in an in-memory **memtable** kept sorted by key. When it grows past a
threshold it is flushed as an immutable, sorted **SSTable** segment. Reads check
the memtable, then segments newest-first, returning the first hit. Deletes write a
**tombstone**. A background **compaction** merges segments (a k-way merge of sorted
runs, newest value wins) and drops tombstones, keeping read amplification bounded.

```
(lsm_put store %k 42)   :: append; auto-flushes when the memtable is full
(lsm_del store %k)      :: writes a tombstone (a later compaction reclaims it)
(lsm_get store %k)      :: -> [%val 42] or %absent
(lsm_compact store)     :: merge all segments into one, dropping tombstones
```

The demo writes five keys with a flush threshold of 2 (so several segments form),
overwrites a key, deletes another, and shows that after compaction the overwrite
wins and the deleted key stays gone — with the segment count dropping to one.

**In an interview**
- *Why is writing faster than a B-tree?* Sequential appends, no in-place page
  updates, no random seeks. Cassandra, RocksDB, LevelDB, HBase all use LSM.
- *What's the catch?* **Read amplification** (a key might live in any segment) and
  **write amplification** (compaction rewrites data repeatedly). You trade read
  latency and background I/O for write throughput.
- *How do reads stay fast?* Keep an index of each SSTable's keys and a **Bloom
  filter** per segment so a read skips segments that definitely lack the key.
- *B-tree vs LSM in one line?* B-tree = read-optimized, in-place, one copy of each
  key; LSM = write-optimized, append-only, compaction reclaims space later.

## Bloom filter — `lib/bloom.lat`

A Bloom filter answers "is `k` in the set?" using a fixed bit array and `k` hash
functions, with **no false negatives** and a tunable false-positive rate, in far
less space than the keys themselves. It is the read accelerator quoted above: an
LSM read consults the per-segment filter and only touches the SSTable when the
filter says "maybe."

The `k` bit positions come from **double hashing** (Kirsch–Mitzenmacher):
`g_i = h1 + i·h2`, so two base hashes give `k` independent-looking probes. Because
Loom has no XOR, the base hashes are polynomial (Horner) hashes finished with a
multiplicative avalanche step, and ranges are taken from the high bits — see
`lib/dhash.lat`.

```
(bloom_addall (bloom_make 128 4) [%apple [%banana 0]])  :: 128 bits, 4 hashes
(bloom_test filter %apple)   :: 0  (possibly present — never a false negative)
(bloom_test filter %grape)   :: 1  (definitely absent)
```

**In an interview**
- *Why no false negatives but false positives?* Adding only ever sets bits; a
  member's bits are all set, so it always tests positive. A non-member can collide
  on already-set bits — a false positive.
- *How many hashes?* The false-positive-minimizing choice is `k = (m/n)·ln 2`, for
  `m` bits and `n` items; the rate is `≈ (1 − e^(−kn/m))^k`. Rough rule: ~10 bits
  per item gives ~1%.
- *Where besides LSM?* Cache/CDN admission, "have I seen this URL," joining huge
  datasets, and (Kleppmann's own work) syncing hash graphs.

---

# Part II — Encoding and evolution (DDIA Ch. 4)

## Varints and schema evolution — `lib/wire.lat`

Two ideas make rolling upgrades safe. **Varint (LEB128)** packs a natural into
7-bit groups with a continuation bit, so small numbers cost one byte — the integer
encoding of Protocol Buffers and Thrift. **Tagged records** give every field a
numeric tag, so a reader using a different schema version can *skip* a field it
doesn't recognize and *default* a field that's missing.

```
(w_varint 300)              :: -> [172 2]   (two bytes; 127 fits in one)
(w_unvarint (w_varint 1000000))             :: -> 1000000  (round-trips)
(w_read writer-record reader-schema)        :: the reader's view of a record
(w_unknown writer-record reader-schema)     :: tags the reader skipped
```

The demo has a writer emit tags 1, 2, and 5; the reader knows tags 1, 2, and a new
tag 3 (default 7). The reader sees 1 and 2, supplies 7 for the missing 3, and
silently skips the unknown 5.

**In an interview**
- *Backward compatibility:* new code reads old data — achieved by giving new fields
  defaults (the missing-field case).
- *Forward compatibility:* old code reads new data — achieved by skipping unknown
  tags (the extra-field case). Tagged formats like Protobuf/Avro/Thrift get both;
  this is why you add fields with new tags and never reuse or renumber old ones.
- *Why varints?* Most integers are small; fixed 32/64-bit fields waste space.

---

# Part III — Replication (DDIA Ch. 5)

When data lives on several nodes, two writes can race and clocks can't be trusted.
These four tools are how leaderless and multi-leader stores stay correct.

## Quorums — `lib/quorum.lat`

With `N` replicas, a write waits for `W` acknowledgements and a read queries `R`.
If **`W + R > N`** the read and write sets are forced to overlap, so a read is
guaranteed to see the latest committed write. Drop below that and reads can be
stale. **Read repair** pushes the freshest value back over the replicas a read
touched, healing them on the read path.

```
(q_overlap 3 2 2)   :: 0 — W+R>N, strong (a read sees the latest write)
(q_overlap 3 1 1)   :: 1 — W+R<=N, a read can miss it
(q_readval (q_write (q_init 3) 2 1 %hi) 2)   :: %hi  (W=2, R=2 overlap)
(q_readval (q_write (q_init 3) 1 1 %hi) 1)   :: %nil (W=1, R=1, stale)
```

**In an interview**
- *Why `W + R > N`?* Pigeonhole: any `W` and any `R` of `N` replicas must share at
  least one, and that shared replica holds the latest write.
- *Common configs?* `W = R = ⌈(N+1)/2⌉` (majority). `W = N, R = 1` for read-heavy;
  `W = 1, R = N` for write-heavy.
- *Does a quorum give you linearizability?* No — concurrent writes, sloppy quorums,
  and read-repair edge cases mean Dynamo-style quorums are still eventually
  consistent. That's what the next two tools address.

## Vector clocks — `lib/vclock.lat`

A version vector maps each node to a counter. Comparing two vectors tells you their
causal relationship: one **happened-before** the other (safe to overwrite), they're
**equal**, or they're **concurrent** — a genuine conflict that must be surfaced
(Dynamo calls the survivors "siblings") rather than silently resolved by a clock.

```
(vc_inc clock %a)            :: a local event on node a
(vc_merge a b)               :: pointwise max — what a replica does on receive
(vc_compare a b)             :: %before | %after | %eq | %concurrent
(vc_conflict a b)            :: 0 if the two versions conflict (are concurrent)
```

**In an interview**
- *Why not wall-clock timestamps?* Clocks skew; "latest by timestamp" silently
  drops a concurrent write. Vector clocks detect concurrency instead of hiding it.
- *Vector clock vs version vector?* Same structure; "version vector" is the
  replication usage (per-replica counters on a data item).
- *Cost?* Size grows with the number of writers; production systems prune old
  entries, which is where the subtle bugs live.

## CRDTs — `lib/crdt.lat`

Conflict-free Replicated Data Types make merge a **join on a lattice**:
commutative, associative, and idempotent. Replicas that have seen the same set of
updates — in any order, with duplicates — reach the same state. That's **strong
eventual consistency**, with no coordination. Four classics are implemented:

```
(gc_value (gc_merge r1 r2))  :: G-Counter: per-node maxes; value = sum
(pn_value pn)                :: PN-Counter: two G-Counters; value = P − N
(gs_merge a b)               :: G-Set: grow-only set; merge = union
(lww_merge a b)              :: LWW-Register: highest timestamp wins
```

The demo merges two counter replicas in both orders (same result — commutative)
and merges a replica with itself (no change — idempotent).

**In an interview**
- *Why commutative/idempotent matters:* gossip delivers updates out of order and
  more than once; only an idempotent, order-independent merge converges anyway.
- *State-based vs operation-based?* These are **state-based** (CvRDTs): ship the
  whole state and join. Op-based ships operations and needs exactly-once causal
  delivery.
- *The catch with LWW?* It resolves conflicts by discarding data; fine for a cache,
  dangerous for a shopping cart (use a set/counter that merges without loss).

## Merkle trees — `lib/merkle.lat`

To check whether two replicas hold the same data without shipping all of it, build
a hash tree: each leaf hashes one key/value, each parent hashes its children. The
**root hash** summarizes everything, so a single comparison says "identical" or
"different." If different, you descend, comparing subtree hashes, and reach exactly
the leaves that disagree — so anti-entropy transfers only the differing keys. This
is how Dynamo, Cassandra, and Riak reconcile replicas in the background.

```
(mk_equal (mk_build kvs-a) (mk_build kvs-b))     :: 0 if the datasets are identical
(mk_diffkeys (mk_build kvs-a) (mk_build kvs-b))  :: the keys that differ
```

**In an interview**
- *Why a tree and not a list of hashes?* The tree turns "find the difference" from
  `O(n)` comparisons into `O(log n + d)` for `d` differing keys — you prune whole
  matching subtrees by one hash check.
- *Where else?* Git (commit/tree objects), blockchains, certificate transparency,
  and ZFS/Btrfs block integrity.

---

# Part IV — Partitioning (DDIA Ch. 6)

## Consistent hashing — `lib/chash.lat`

Sharding by `hash(key) mod N` remaps almost every key when `N` changes — a
re-partition storm. Consistent hashing places keys *and* nodes on one ring; a key
is owned by the first node clockwise from it. Adding or removing a node moves only
the keys in that one arc — about `1/N` of them. **Virtual nodes** (several ring
points per physical node) even out the load and the movement.

```
(ch_build [%n1 [%n2 [%n3 0]]] 16 65536)   :: 3 nodes, 16 vnodes each, ring 0..65536
(ch_lookup ring %key 65536)                :: which node owns the key
(ch_moved assignment-before assignment-after)  :: how many keys changed owner
```

The demo assigns 20 keys to 3 nodes, then adds a 4th: only ~5 keys move (not 20),
and removing a node moves only the keys that node owned.

**In an interview**
- *Why virtual nodes?* Without them, three nodes split the ring into three uneven
  arcs and load is lumpy; many small vnodes approximate a uniform split and let you
  weight bigger machines with more points.
- *Consistent hashing vs a lookup table?* DynamoDB/Cassandra-style consistent
  hashing needs no central directory; systems like Spanner instead keep an explicit
  partition map. Trade decentralization for control.

---

# Part V — Time, order, and consensus (DDIA Ch. 8–9)

## Lamport clocks — `lib/lamport.lat`

A logical clock: a counter per node, incremented on every event, and advanced to
`max(local, received) + 1` on receiving a message. The result is consistent with
causality — if `a` causally precedes `b` then `L(a) < L(b)` — and ties broken by
node id give a **total order**, the foundation of total-order broadcast.

```
(lam_tick clock)        :: a local event; the new value is the timestamp
(lam_recv clock ts)     :: advance past a message's timestamp, then tick
(lam_order events)      :: sort [ts node] events into one agreed total order
```

**In an interview**
- *Lamport vs vector clock?* A Lamport clock gives a total order but **cannot**
  tell you whether two events were concurrent; a vector clock can, at higher cost.
  Use Lamport when you just need *an* order everyone agrees on.
- *Relation to consensus?* Total-order broadcast is equivalent to consensus; Lamport
  timestamps order messages but don't by themselves decide membership or handle
  failures — that's Raft/Paxos territory.

---

# Part VI — Batch processing (DDIA Ch. 10)

## MapReduce — `lib/mapred.lat`

Three pure stages. **Map** turns each input record into zero or more `[key value]`
pairs. **Shuffle** (the framework's job) groups pairs by key. **Reduce** collapses
each key's values to a result. Because every stage is a deterministic function over
immutable input, the job is restartable and embarrassingly parallel.

```
(mr_run mapf redf input)   :: the generic pipeline
(mr_wordcount docs)        :: the canonical example: word -> total count
```

**In an interview**
- *Why pure functions?* Determinism makes a failed task safe to retry on another
  machine — the basis of fault tolerance in batch systems.
- *Why is shuffle the expensive part?* It moves all values for a key across the
  network to one reducer; skewed keys ("hot keys") are the classic bottleneck.
- *Successors?* Spark keeps intermediate data in memory and models the job as a DAG
  rather than rigid map→reduce rounds, but the map/shuffle/reduce shape remains.

---

# Part VII — Orpheus *is* a data-intensive system

These techniques aren't decoration. The host implements the same ideas to make
ordinary Latte apps durable and distributed for free:

- **A log-structured, content-addressed event store** (`src/store.rs`,
  `src/agent.rs`). Every change is an append to an event log keyed by its hash —
  the same append-only, immutable-segment discipline as the LSM-tree above, with
  snapshots playing the role of compaction. The `store::tests` cover log replay and
  snapshot migration.
- **Gossip and anti-entropy.** Linked machines exchange events and converge; that
  reconciliation is exactly the Merkle/quorum reasoning in Parts III, and the
  Forge (team coding) is a shared, gossiped snippet log built on it.
- **Logical time.** Orpheus orders events by `(lamport, node, hash)` — a Lamport
  timestamp with deterministic tie-breaks, precisely Part V.
- **Convergent state.** Because Mocha pokes are commutative, durable events, app
  state converges without a coordinator — the CRDT story in Part III. A `poke` is a
  durable, gossiped event; persistence, time-travel, and log compaction come for
  free (see `lib/mocha.lat`).

So when you call `(lww_merge …)` or `(mk_diffkeys …)`, you're rehearsing, in the
small, what the runtime under your program does in the large.

---

# Part VIII — The rest of the system, at a glance

So this guide stays balanced rather than DDIA-only, here is the whole stack and
where to read more. Everything above the host is written *in Latte*.

- **Loom** — the 12-rule, Nock-style virtual machine; the only datatype is the
  **Knot** (an atom or a cell). Everything compiles to it. (`src/loom.rs`)
- **Latte** — the functional language these libraries are written in; arbitrary-
  precision naturals, loobean logic, gates, tail-recursive `loop`. Reference:
  `latte-language.md`; writing/registering libraries: `adding-libraries.md`.
- **Facet** — the markup language that embeds live tool calls in text
  (`facet-language.md`).
- **Hymn** — the HTTP server hosting the GUI and the `/api/*` endpoints
  (`src/serve.rs`).
- **The System (GUI)** — Oberon-style tracks of viewers over editable texts with
  embedded objects: charts, drawings, renders, panels. Mark a frame, run commands
  in place, compile modules live. Full guide: `the-system.md`.
- **The Formatter** — `Edit.Format *` formats the **marked** source frame,
  provably meaning-preserving (the result must re-parse to the same AST). How to
  mark and format is in `the-system.md`.
- **SCArs / Conlang** — a sound-change applier and phonology builder (the
  linguistics suite); `scars-sound-changes.md` and `conlang-tools.md`.
- **The numeric stack** — signed fixed-point `num`, n-dimensional `tensor`,
  `ml`/`nn` training, `plot` visualization, `fin`/`ta` finance
  (`visualization-and-ml.md`).
- **Mocha** — the durable, gossiped application runtime (`lib/mocha.lat`); the
  planner is in `planning.md`.
- **Interaction nets** — an alternative reduction engine (`interaction-nets.md`).
- **Building & running** — toolchain, tests, and the CLI surface:
  `building-and-running.md`.

Each data-intensive technique is one Latte module in `lib/`, with arms namespaced
so they coexist in one flat scope: `dh_` (hashing), `bloom_`, `lsm_`, `bt_`,
`vc_` (vclock), `gc_`/`pn_`/`gs_`/`lww_` (CRDTs), `lam_` (lamport), `ch_` (chash),
`q_` (quorum), `w_` (wire), `mr_` (mapred), `mk_` (merkle). Read any of them with
`System.Open <name>` in the GUI or `latte eval --lib name=lib/name.lat "…"` from
the shell.
