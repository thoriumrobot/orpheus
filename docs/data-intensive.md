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
  `mapred`, `merkle`, `mvcc`, `hll`, `cms`, `stream`, `raft`) runs one.
- **Console / `eval`:** every library is in the default scope, so
  `latte eval "(bloom_test (bloom_add (bloom_make 64 3) %x) %x)"` just works.
- **GUI:** open the System page, middle-click a `… ` command line, or open a
  module from the Modules viewer (`System.Open lsm`) to read and edit the source.

**Every example below is a runnable one-liner.** A name like `tree` or `store` is
not bound to anything — typing `(bt_search tree %kk)` on its own gives
`unbound face 'tree'`, because the value built on the previous line was never named.
So each library ships a **`<name>_sample`** fixture returning a ready-made instance;
pass it a dummy argument and drop it straight into any call:

```
(bt_search (bt_sample 0) %kk)       :: -> [%val 5]   — no setup, runs as-is
(lsm_get   (lsm_sample 0) %cherry)  :: -> [%val 33]
```

To build and reuse your own value in one expression, bind it with `let … in`:

```
let tree = (bt_build [[%mm 1] [[%kk 5] 0]] 2) in (bt_search tree %kk)
```

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
(bt_sample 0)               :: a ready-made tree of 8 keys (t=2)
(bt_search (bt_sample 0) %kk)        :: -> [%val 5]   (0 if absent)
(bt_inorder (bt_sample 0))           :: sorted [key value] list — a full index scan
(bt_range (bt_sample 0) %cc %mm)     :: every [key value] with cc <= key <= mm
(bt_min (bt_sample 0)) (bt_max (bt_sample 0)) :: smallest / largest entry
(bt_height (bt_sample 0))            :: every leaf is this far from the root
(bt_delete (bt_sample 0) %mm 2)      :: remove a key (borrow/merge to stay balanced)
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
- *How do range scans work?* On the sorted order: find the low key, then walk
  in-order until you pass the high key (`bt_range` here). A production engine uses
  a **B+-tree** — values live only in the leaves and the leaves are linked in a
  list — so a range scan is "seek once, then follow leaf pointers," never
  revisiting internal nodes. (This library keeps values in every node for brevity
  and rides the in-order traversal; the complexity story is the same.)
- *Deletion?* The mirror of insertion, and implemented here as `bt_delete`: if
  removing a key would underflow a node (`< t-1` keys) you **borrow** a key from a
  sibling (rotating it through the parent), or, if both siblings are minimal,
  **merge** the node, a sibling, and their separator key into one node and recurse.
  Like insertion it stays a single root-to-leaf pass by fixing each child to hold
  `>= t` keys *before* descending into it; when the root loses its last key its only
  child becomes the new root (the one way height shrinks). It is the fiddly half of
  the B-tree, which is why copy-on-write B-trees (which rewrite a path instead of
  editing in place) are popular for snapshots and crash-safety.

## LSM-tree / SSTable — `lib/lsm.lat`

An LSM-tree optimizes the opposite way: never seek to update, only append. Writes
land in an in-memory **memtable** kept sorted by key. When it grows past a
threshold it is flushed as an immutable, sorted **SSTable** segment. Reads check
the memtable, then segments newest-first, returning the first hit. Deletes write a
**tombstone**. A background **compaction** merges segments (a k-way merge of sorted
runs, newest value wins) and drops tombstones, keeping read amplification bounded.

```
(lsm_sample 0)                  :: ready-made store: 5 writes, an overwrite, a delete
(lsm_put (lsm_sample 0) %k 42)  :: append; auto-flushes when the memtable is full
(lsm_del (lsm_sample 0) %k)     :: writes a tombstone (a later compaction reclaims it)
(lsm_get (lsm_sample 0) %cherry)        :: -> [%val 33] (the overwrite) or %absent
(lsm_compact (lsm_sample 0))            :: merge all segments into one, dropping tombstones
(lsm_keys (lsm_sample 0))               :: live keys, sorted (merged view across layers)
(lsm_range (lsm_sample 0) %apple %elder):: sorted [key value] within the range
(lsm_segmeta (lsm_sample 0))            :: per segment, [minKey [maxKey count]] — the zone map
(lsm_probed (lsm_sample 0) %zzz)        :: -> 0: the zone map skips every run for an out-of-range key
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
- *How do reads stay fast?* Three layers of skipping. A **zone map** — the min/max
  key per segment, which `lsm_segmeta` exposes — lets a read ignore any segment that
  can't hold the key. This library wires that into the read path: `lsm_get` skips any
  run whose `[min,max]` excludes the key, and `lsm_probed` reports how many runs a
  read actually scans (an out-of-range key touches zero). A production engine adds two
  more: a **sparse index** (one key every few KB) to binary-search to the right block
  within a segment, and a per-segment **Bloom filter** (the `bloom` library) to answer
  "is `k` in this segment?" with a few bit tests before any I/O. Range reads
  (`lsm_range`) here still do a sorted merge across all layers then filter to the
  range; skipping non-overlapping segments by zone is the same idea applied to scans.
- *Size-tiered vs leveled compaction?* Size-tiered merges similarly-sized runs
  (fewer, bigger merges — write-friendly, more space and read amplification);
  leveled keeps each level a sorted, non-overlapping set ~10× the one above (more
  merging — read- and space-friendly). The choice is the classic LSM knob.
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
(bloom_sample 0)                       :: ready-made filter (256 bits, 3 hashes, 3 members)
(bloom_test (bloom_sample 0) %apple)   :: 0  (possibly present — never a false negative)
(bloom_test (bloom_sample 0) %grape)   :: 1  (definitely absent)
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
(w_read (w_sample 0) (w_sample_schema 0))   :: the reader's view (tag 3 defaults to 7)
(w_unknown (w_sample 0) (w_sample_schema 0)):: tags the reader skipped (tag 5)
(w_zigzag [1 5])            :: signed -5 -> 9   (zigzag: 0,-1,1,-2 -> 0,1,2,3)
(w_decrec (w_encrec (w_sample 0)))          :: round-trip length-delimited framing
```

The demo has a writer emit tags 1, 2, and 5; the reader knows tags 1, 2, and a new
tag 3 (default 7). The reader sees 1 and 2, supplies 7 for the missing 3, and
silently skips the unknown 5.

**Signed integers — zigzag.** A varint encodes a natural, so a naive signed
encoding would store `-1` as a maximal ten-byte value. **Zigzag** interleaves the
signs — `0, -1, 1, -2, 2 → 0, 1, 2, 3, 4` — so small magnitudes of either sign stay
one byte, then a plain varint finishes the job (`w_zigzag` / `w_unzigzag`).

**Length-delimited framing.** Real forward compatibility needs more than knowing the
tags: to *skip* an unknown field you must know where it ends. So each field is
written as `varint(tag) ++ varint(length) ++ bytes` (`w_field`, `w_encrec`). A
reader with no schema at all can still parse the stream into `[tag bytes]` pairs and
carry unknown fields through untouched (`w_decrec`) — and because the payload is
just bytes, records nest.

**In an interview**
- *Backward compatibility:* new code reads old data — achieved by giving new fields
  defaults (the missing-field case).
- *Forward compatibility:* old code reads new data — achieved by skipping unknown
  tags by **length** (the framing above). Tagged formats like Protobuf/Avro/Thrift
  get both; this is why you add fields with new tags and never reuse or renumber old
  ones.
- *Protobuf vs Avro?* Protobuf tags each field on the wire (self-describing framing,
  as here). Avro writes no tags at all — just values — and resolves the **writer's
  schema** against the **reader's schema** at decode time, which is why Avro needs
  the writer schema available (great for big files and a schema registry) while
  Protobuf is friendlier for loosely-coupled RPC.
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
(vc_inc (vc_new 0) %a)       :: a local event on node a -> [[%a 1] 0]
(vc_merge (vc_inc (vc_new 0) %a) (vc_inc (vc_new 0) %b))   :: pointwise max on receive
(vc_compare (vc_inc (vc_new 0) %a) (vc_inc (vc_new 0) %b)) :: -> %concurrent
(vc_conflict (vc_inc (vc_new 0) %a) (vc_inc (vc_new 0) %b)):: 0 — they conflict (concurrent)
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
(gc_value (gc_merge (gc_inc (gc_new 0) %a 5) (gc_inc (gc_new 0) %b 7)))  :: G-Counter -> 12
(pn_value (pn_dec (pn_inc (pn_new 0) %a 10) %b 3))   :: PN-Counter: P − N -> 7
(gs_merge [%x [%y 0]] [%y [%z 0]])                   :: G-Set: union of two replicas
(lww_value (lww_merge (lww_set 5 %red %n1) (lww_set 9 %blue %n2)))  :: LWW -> %blue (ts9 wins)
```

The demo merges two counter replicas in both orders (same result — commutative)
and merges a replica with itself (no change — idempotent).

**The hard part: removal and concurrent writes.** A grow-only set can't delete and
LWW throws data away, so the library also implements the two CRDTs that handle
concurrency *without loss*:

- **OR-Set** (observed-remove set). Every add stamps the element with a unique tag
  (a "dot"); a remove tombstones only the tags it has actually *seen*. An element is
  present if any add-tag survives. So when one replica removes `x` while another
  concurrently re-adds it, the merge keeps `x` — the add the remove never observed
  wins. That is the behaviour a naive add/remove set (2P-Set) gets wrong.
- **MV-Register** (multi-value register), the Dynamo "siblings" model. A write is
  stamped with a vector clock and supersedes only the values it causally dominates;
  merge keeps the causally-**maximal** values. Two concurrent writes therefore both
  survive as siblings for the application to resolve, and a later write that has
  seen both collapses them back to one.

```
:: OR-Set: a concurrent re-add survives a remove that never observed it (0 = present)
(or_has (or_merge (or_remove (or_add (or_new 0) %x [%n1 1]) %x)
                  (or_add (or_new 0) %x [%n2 1])) %x)
:: MV-Register: two concurrent writes are kept as siblings -> [%red, %blue]
(mv_values (mv_merge (mv_set (mv_new 0) %red (vc_inc (vc_new 0) %a))
                     (mv_set (mv_new 0) %blue (vc_inc (vc_new 0) %b))))
```

**In an interview**
- *Why commutative/idempotent matters:* gossip delivers updates out of order and
  more than once; only an idempotent, order-independent merge converges anyway.
- *State-based vs operation-based?* These are **state-based** (CvRDTs): ship the
  whole state and join. Op-based ships operations and needs exactly-once causal
  delivery.
- *The catch with LWW?* It resolves conflicts by discarding data; fine for a cache,
  dangerous for a shopping cart — use an OR-Set or a counter that merges without
  loss.
- *Why does the OR-Set need tags?* Without them "add then remove" and "remove then
  add" are indistinguishable after merge. The unique tag records *which* add a
  remove cancelled, so a concurrent add (with a tag the remove never saw) survives.
- *Cost?* Tombstones and dots accumulate; real systems prune them once causally
  stable (every replica has observed them) — the same pruning problem as version
  vectors.

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
(ch_sample 0)                              :: ready-made ring: 3 nodes, 16 vnodes each
(ch_lookup (ch_sample 0) %key 65536)       :: which node owns the key
(ch_replicas (ch_sample 0) %key 3 65536)   :: the 3 distinct nodes that replicate key
(rv_lookup [%n1 [%n2 [%n3 0]]] %key)       :: rendezvous (HRW) owner — no ring needed
```

The demo assigns 20 keys to 3 nodes, then adds a 4th: only ~5 keys move (not 20),
and removing a node moves only the keys that node owned.

**Replica placement (the preference list).** A key isn't stored on one node but
replicated to the next `N` *distinct physical* nodes clockwise — Dynamo's
"preference list" (`ch_replicas`). Walking distinct nodes (not raw ring points)
matters: it stops two replicas of the same key landing on two vnodes of the same
machine, which would defeat the replication.

**Rendezvous (HRW) hashing.** An alternative with no ring at all: score every
(node, key) pair with a hash and send the key to the highest-scoring node
(`rv_lookup`); the top-`k` scores give the replica set directly (`rv_topn`). It has
the same minimal-movement property — drop a node and only *its* keys re-home, to
their next-highest scorer — with simpler state and naturally even load.

**In an interview**
- *Why virtual nodes?* Without them, three nodes split the ring into three uneven
  arcs and load is lumpy; many small vnodes approximate a uniform split and let you
  weight bigger machines with more points.
- *Consistent hashing vs rendezvous?* Both move ~`1/N` of keys on a membership
  change. The ring needs vnodes to balance load and a sorted structure to find the
  successor; HRW needs neither (it scores all nodes, `O(N)` per lookup) and gives
  the replica list for free — great when `N` is modest, as in a shard router.
- *Consistent hashing vs a lookup table?* DynamoDB/Cassandra-style consistent
  hashing needs no central directory; systems like Spanner instead keep an explicit
  partition map. Trade decentralization for control.

---

# Part V — Transactions and isolation (DDIA Ch. 7)

A transaction promises atomicity and isolation, but "isolation" has levels, and the
interesting question is always *which anomalies does this level allow?* The dominant
modern answer is **snapshot isolation** built on multi-version storage.

## MVCC and snapshot isolation — `lib/mvcc.lat`

Multi-Version Concurrency Control never overwrites: every write creates a new
version stamped with its commit timestamp. A transaction takes a **snapshot** — its
start timestamp — and for every key reads the newest version committed at or before
that instant. So **readers never block writers** (they read an older version) and a
transaction sees a single consistent point in time for its whole life (repeatable
read). On commit, a **write-write check** rejects a transaction whose keys were
committed by someone else since it began — *first committer wins*.

```
(mvcc_sample 0)                          :: ready-made store: x=1@ts5, then x=2@ts15
(mvcc_read (mvcc_sample 0) %x 10)         :: snapshot read at ts 10 -> [%val 1]
(mvcc_read (mvcc_sample 0) %x 20)         :: snapshot read at ts 20 -> [%val 2]
(head (mvcc_try (mvcc_sample 0) [[%x 9] 0] 10 18))      :: %abort — x changed in (10,18]
(head (mvcc_try (mvcc_sample 0) [[%y 9] 0] 10 18))      :: %commit — y untouched
:: serializable: read-set validation aborts write skew that plain SI would allow
(head (mvcc_ssi_try (mvcc_sample 0) [%x 0] [[%y 9] 0] 10 18))  :: %abort — read x changed
```

The demo commits `x=1` at ts 5 and `x=2` at ts 15. A reader on a snapshot at ts 10
still sees `1` (the later commit is invisible); a snapshot at ts 20 sees `2`. A
transaction that started at ts 10 and tries to write `x` at commit ts 18 **aborts**
(because `x` changed at ts 15, inside its window); writing an untouched key commits.

**In an interview**
- *What anomaly does SI still allow?* **Write skew.** Two transactions read an
  overlapping set, each checks an invariant that currently holds, and each writes a
  *different* row. Their write sets are disjoint, so the write-write check passes for
  both — yet together they violate the invariant (the classic "both on-call doctors
  go off shift" bug). Snapshot isolation does not prevent it.
- *So how do you get serializable?* Three routes: **actual serial execution**
  (VoltDB/Redis — one transaction at a time on a partition); **two-phase locking**
  (predicate/range locks — correct but contended); or **serializable snapshot
  isolation (SSI)**, which runs optimistically on a snapshot and aborts a transaction
  only when it detects the read-write dependency that would make the schedule
  non-serializable (PostgreSQL's `SERIALIZABLE`, the modern favourite). `mvcc_ssi_try`
  implements the optimistic core: alongside the write-write check it **validates the
  read set** — if any key the transaction read was overwritten by a commit during its
  lifetime, its decision rested on stale data and it aborts. That is exactly what
  turns the write-skew example above from a commit into an abort.
- *Why timestamps for visibility?* A version is visible iff `commit_ts <= snapshot`,
  so visibility is a pure comparison — no locks on the read path. The hard part is
  garbage-collecting versions no live snapshot can still see.
- *Relation to undo logs / time travel?* The version chain *is* an undo history;
  many engines expose it as "query as of timestamp T."

---

# Part VI — Time, order, and consensus (DDIA Ch. 8–9)

## Lamport clocks — `lib/lamport.lat`

A logical clock: a counter per node, incremented on every event, and advanced to
`max(local, received) + 1` on receiving a message. The result is consistent with
causality — if `a` causally precedes `b` then `L(a) < L(b)` — and ties broken by
node id give a **total order**, the foundation of total-order broadcast.

```
(lam_tick 0)            :: a local event; the new value is the timestamp -> 1
(lam_recv 3 5)          :: advance past a message's timestamp, then tick -> 6
(lam_order [[5 %a] [[2 %b] [[5 %c] 0]]])  :: sort [ts node] events into a total order
```

**In an interview**
- *Lamport vs vector clock?* A Lamport clock gives a total order but **cannot**
  tell you whether two events were concurrent; a vector clock can, at higher cost.
  Use Lamport when you just need *an* order everyone agrees on.
- *Relation to consensus?* Total-order broadcast is equivalent to consensus; Lamport
  timestamps order messages but don't by themselves decide membership or handle
  failures — that's Raft/Paxos territory.

## Raft consensus — `lib/raft.lat`

Consensus is how a set of replicas agree on one order of operations — a replicated
log — despite crashes and lost messages. Raft is the readable member of the family,
and its safety rests on three rules, implemented here as pure functions:

1. **Election restriction.** A server grants its vote only to a candidate whose log
   is at least as up-to-date as its own (higher last term wins; same term, longer
   log wins — `raft_uptodate`). This guarantees a new leader already holds every
   committed entry, so leadership never loses data.
2. **Log matching.** `AppendEntries` is rejected unless the entry just before the new
   ones matches the leader in **both index and term** (`raft_append`). On acceptance
   any conflicting suffix is truncated and replaced. Inductively, identical
   `(index, term)` implies identical logs up to that point.
3. **Commit by majority.** An entry is committed once it is stored on a **majority**
   of servers (`raft_commit` scans the per-follower match indices for the highest
   index a majority has reached). A leader only counts entries from its *own* term.

```
(raft_uptodate 2 3 2 2)        :: 0 — candidate's log (term2,idx3) is OK to elect over (term2,idx2)
(raft_grantvote 1 0 %c1 2 2 3 1 2)  :: 0 = grant the vote (see lib for arg order)
(raft_append (raft_sample 0) 3 2 [[3 %d] 0])  :: append after a matching entry -> [%ok newlog]
(raft_commit [3 [3 [1 0]]] 3)  :: highest index replicated on a majority of 3 -> 3
```

The demo shows a candidate with a longer but lower-term log being refused (the
election restriction), an `AppendEntries` accepted on a matching prefix and rejected
on a mismatch, and a commit index of 2 from match indices `[3 3 2 1 1]` across five
servers (only two servers reached index 3, but three reached index 2).

**In an interview**
- *Why a leader at all?* A single leader sequences all writes, turning consensus per
  entry into "replicate the leader's log." The price is an election whenever the
  leader fails, gated by randomized timeouts to avoid split votes.
- *Raft vs Paxos vs Zab?* Same safety guarantees; Raft factors the problem into
  leader election + log replication + safety to be teachable, Multi-Paxos is the
  classic, Zab is ZooKeeper's. All need a majority quorum and so tolerate `f`
  failures with `2f+1` nodes.
- *Why majority?* Any two majorities of `2f+1` intersect, so a newly elected leader's
  voting majority necessarily overlaps the majority that stored any committed entry —
  the entry can't be lost. This is the same quorum-overlap idea as `W + R > N`.
- *Liveness vs safety?* Raft is always safe (never returns a wrong result) but can
  stall during partitions/repeated elections — exactly what FLP predicts; randomized
  timeouts restore progress in practice.

---

# Part VII — Batch processing (DDIA Ch. 10)

## MapReduce — `lib/mapred.lat`

Three pure stages. **Map** turns each input record into zero or more `[key value]`
pairs. **Shuffle** (the framework's job) groups pairs by key. **Reduce** collapses
each key's values to a result. Because every stage is a deterministic function over
immutable input, the job is restartable and embarrassingly parallel.

```
(mr_wordcount (mr_sample 0))   :: word -> total count over two sample docs (the -> 2)
:: the generic pipeline: map each doc to [key value] pairs, group, then reduce
(mr_run (fn [d] -> (mr_wc_map d)) (fn [k vs] -> (mr_wc_red k vs)) (mr_sample 0))
```

**In an interview**
- *Why pure functions?* Determinism makes a failed task safe to retry on another
  machine — the basis of fault tolerance in batch systems.
- *Why is shuffle the expensive part?* It moves all values for a key across the
  network to one reducer; skewed keys ("hot keys") are the classic bottleneck.
- *Successors?* Spark keeps intermediate data in memory and models the job as a DAG
  rather than rigid map→reduce rounds, but the map/shuffle/reduce shape remains.

---

# Part VIII — Stream processing (DDIA Ch. 11)

Batch processing assumes a finite input; a stream is unbounded, so aggregates have
no natural end. The fix is to cut the stream into **windows over event time** and to
reason explicitly about events that arrive out of order.

## Windowing and watermarks — `lib/stream.lat`

A **tumbling** window of size `s` assigns an event at time `t` to window
`floor(t/s)`; the values in each window are aggregated (count, sum). A **sliding**
window emits overlapping windows every `step`. Because events can arrive late, a
**watermark** marks "event time up to here has probably arrived" — anything older is
**late** and is dropped or routed to a side output. This is the event-time model of
Flink, Beam, and Kafka Streams.

```
(stream_sums (stream_sample 0) 10)     :: [windowId sum] for tumbling windows of size 10
(stream_counts (stream_sample 0) 10)    :: [windowId count]
(stream_late (stream_sample 0) 12)      :: events older than the watermark (arrived too late)
(stream_slide_sum (stream_sample 0) 10 5) :: sliding windows: size 10, step 5
```

The demo windows five timestamped events into size-10 buckets (summing and
counting), separates the two events that fall before a watermark at time 10, and
shows the overlapping sliding-window sums.

**In an interview**
- *Event time vs processing time?* Event time is when something happened; processing
  time is when your job saw it. Windowing on event time gives correct, reproducible
  results regardless of delays; processing time is simpler but wrong under lag.
- *What's a watermark, really?* A moving lower bound on event time. When it passes a
  window's end you can emit that window's result and reclaim its state — trading a
  little latency (waiting for stragglers) against completeness.
- *Stream–table duality.* A table is the current state; a stream is the log of
  changes to it. Replaying the change stream rebuilds the table (event sourcing /
  change data capture), and a windowed aggregate of a stream *is* a table — the
  insight behind Kafka/Samza and behind Orpheus's own event log.
- *Exactly-once?* Achieved not by delivering once but by making effects idempotent or
  transactional (dedup keys, atomic offset+output commit), so a replay after failure
  lands the same result.

---

# Part IX — Probabilistic structures for analytics

Analytics over huge or unbounded data often can't afford exact counts. Two sketches
answer the two most common questions in sublinear, fixed memory, trading a bounded
error for enormous space savings — and both are mergeable, so per-shard sketches
combine into a global one.

## HyperLogLog — `lib/hll.lat`

**How many DISTINCT?** Exact distinct-count needs memory proportional to the number
of distinct items. HyperLogLog keeps `m = 2^p` small registers: hash each item, use
the first `p` bits to pick a register, and store the largest "leading-zero count +
1" seen there. A run of `k` leading zeros appears once per `2^k` items, so the
register maxima estimate the cardinality. The standard error is about
`1.04/√m` — billions of items in a few kilobytes.

```
(hll_add (hll_new 8) %item)   :: p=8 -> 256 registers; add one item
:: feed 400 distinct keys and estimate the count (~400; here ~422)
(hll_count (hll_addall (hll_new 8) (map (fn [i] -> (cat %k (numtext i))) (range 400))))
(hll_zeros (hll_new 8))       :: empty registers — a fill gauge (256 when fresh)
```

The demo feeds 400 distinct keys into a 256-register sketch and estimates ~400
(here 422, within the expected error band), unchanged when the same items are added
again — distinct-count, not a total.

**Implementation notes (worth knowing).** Two integer-only subtleties bite in
practice and are handled here. First, the estimator assumes the hash spreads items
*randomly*; plain multiplicative hashing maps a sequence like `k1, k2, …` to a
**low-discrepancy** (anti-clustered) spread — wonderful for a hash table, wrong for
HLL — so a **non-linear** finalizer (a squaring step) is applied to restore
Poisson-distributed collisions. Second, the raw estimate is badly biased when many
registers are still empty, so for small cardinalities the library switches to
**linear counting** (`m·ln(m/zeros)`, with a fixed-point logarithm) — exactly the
small-range correction the original HLL paper prescribes. (Production HLL++ adds
empirical bias correction and sparse storage on top.)

**In an interview**
- *Why not a hash set?* A set is exact but `O(distinct)` memory; HLL is approximate
  but `O(2^p)` fixed — constant memory for any cardinality.
- *Why is it mergeable?* The registers form a max-lattice: the union's register is the
  pairwise max, so two shards' sketches merge with no re-scan — ideal for
  map-reduce / per-partition rollups.
- *Tune accuracy?* Raise `p`: error ≈ `1.04/√(2^p)`, at `2^p` registers. `p = 14`
  (16384 registers, ~16 KB) is Redis's default for ~0.8% error.

## Count-Min Sketch — `lib/cms.lat`

**How OFTEN?** A Count-Min Sketch estimates the frequency of a key using `d` rows of
`w` counters and `d` hash functions. Adding a key increments one counter per row; the
estimate is the **minimum** of those counters. Because a hash collision can only
*add* to a counter, the sketch **never under-counts** — it is an upper bound, tight
for the heavy hitters that dominate traffic.

```
(cms_add (cms_new 4 64) %k)   :: 4 rows of 64 counters; add one key
:: add a five times, b twice, then read a's estimated count -> 5
(cms_count (cms_addall (cms_new 4 64) [%a [%a [%a [%a [%a [%b [%b 0]]]]]]]) %a)
```

The demo adds `a` five times, `b` twice, `c` once, and reads those counts back
exactly (no collisions at this size), while an unseen key reads 0.

**In an interview**
- *Why the minimum?* Every row's counter for `k` includes `k`'s true count plus any
  collisions; the least-collided row is the tightest, so the min is the best upper
  bound. Error is `≤ ε·total` with probability `1 − δ` for `w = ⌈e/ε⌉`,
  `d = ⌈ln(1/δ)⌉`.
- *HLL vs CMS in one line?* HLL = how many distinct (cardinality); CMS = how often
  each (frequency). Different questions, same "small sketch, bounded error, merges
  cleanly" philosophy — and CMS feeds streaming top-`k` / heavy-hitter detection.
- *Where used?* Network flow accounting, trending queries, and cache admission
  (TinyLFU/W-TinyLFU in Caffeine uses a Count-Min Sketch).

---

# Part IX½ — Putting it together: a composed database — `lib/db.lat`

The libraries above are the *parts* of a database. `db.lat` wires them into one
small engine so you can watch the techniques cooperate — it is the through-line of
Parts I–VII in a single module. A `db` value is the tuple
`[ store index bloom idxtag rschema ts vers ]`, and each field is a layer:

| Layer | Library | DDIA |
|---|---|---|
| primary key → record | LSM-tree (`lsm`) | Ch. 3 (storage) |
| "definitely absent?" before any read | Bloom filter (`bloom`) | Ch. 3 (negative lookups) |
| indexed field value → primary keys | a second LSM (`lsm`) | Ch. 3 (secondary index) |
| tagged records, evolved on read | `wire` | Ch. 4 (encoding/evolution) |
| key → shard | consistent hashing (`chash`) | Ch. 6 (partitioning) |
| read-set validation at commit | the version map `vers` | Ch. 7 (OCC / SSI) |

Records are wire-style field lists (`[ [tag value] … 0 ]`). A read passes the
record through the reader schema, so a row written before a field existed still
loads with that field's default — schema evolution, live. The Bloom filter
shortcuts a guaranteed miss so a lookup for an absent key touches no run at all.
The secondary index keeps a posting list of primary keys per field value, updated
on every put and delete. And a transaction reads a consistent snapshot — free,
because a `db` value is immutable — then commits only if nothing it *read* changed
underneath it; that read-set validation is what turns snapshot isolation
serializable and refuses write skew.

`db_sample` is a ready database (three users, indexed on a "city" field), so every
example runs as a one-liner:

```
(db_get (db_sample 0) %u1)                 :: primary read -> [%rec [[1 %alice] [[2 %nyc] 0]]]
(db_get (db_sample 0) %u9)                 :: %absent — and the Bloom filter skipped the store
(db_skipped (db_sample 0) %u9)             :: 1 = the read was skipped entirely
(map (fn [r] -> (head r)) (db_query (db_sample 0) %nyc))   :: secondary index -> [%u3 %u1]
(map (fn [r] -> (head r)) (db_query (db_delete (db_sample 0) %u1) %nyc))  :: delete updates the index -> [%u3]
(map (fn [e] -> (head e)) (db_range (db_sample 0) %u1 %u2)):: primary range scan -> [%u1 %u2]
:: schema evolution: a row with no field 3, read through a schema that expects one
(db_get (db_put (db_open 2 [[1 0] [[2 0] [[3 %zz] 0]]] 4) %u1 [[1 %alice] [[2 %nyc] 0]]) %u1)
(db_route (ch_build [%n1 [%n2 [%n3 0]]] 16 65536) %u1 65536)   :: which shard owns u1
:: a transaction commits while its read set holds...
(head (db_commit (db_sample 0) (db_sample 0) [%u1 0] [[%u4 [[1 %dave] 0]] 0]))  :: %commit
:: ...but aborts once a key it read has changed (write skew prevented)
(let snap = (db_sample 0) in let cur = (db_put snap %u1 [[1 %alx] 0]) in
  (head (db_commit cur snap [%u1 0] [[%u2 [[1 %bz] 0]] 0])))                    :: %abort
```

Run the whole demo with `latte ddia db`.

**In an interview**
- *Why is a snapshot free here?* Because the database is an immutable value:
  "begin" just keeps a reference to the current `db`, and any number of readers
  share it without locks. A real engine recreates this with MVCC version chains.
- *What does the Bloom filter buy?* It converts most negative lookups — the common
  case for a write-heavy key space — into a single bit test instead of a read
  across every sorted run. The cost is a tunable false-positive rate (a deleted key
  may still test "possibly present", costing one wasted read, never a wrong answer).
- *Where would this engine differ from production?* The layers here are honest but
  separate; a real LSM unifies the value store, secondary index, and version
  metadata into shared segments, and commit validation runs against a concurrency
  manager rather than a per-key version map. The *reasoning*, though, is identical.

---



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
  timestamp with deterministic tie-breaks, precisely Part VI.
- **Convergent state.** Because Mocha pokes are commutative, durable events, app
  state converges without a coordinator — the CRDT story in Part III. A `poke` is a
  durable, gossiped event; persistence, time-travel, and log compaction come for
  free (see `lib/mocha.lat`).

So when you call `(lww_merge …)` or `(mk_diffkeys …)`, you're rehearsing, in the
small, what the runtime under your program does in the large.

---

# Part XI — The rest of the system, at a glance

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
- **Definition lookup (`System.Def`)** — highlight a function name anywhere in the
  GUI and click **≡ Def** (or run `System.Def`); the host runs the `lookup` library
  (`lk_lookup`) over every module's source and shows the function's doc comment and
  body. The lookup itself is a Latte tool: it treats a module as a list of line
  cords and matches on a line's byte prefix, so finding `  name` followed by a space
  or `=` is just `line mod 256^k`. Try it from the shell, too:
  `latte ddia lookup`, or `curl 'localhost:PORT/api/defn?name=bt_search'`.
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
`q_` (quorum), `w_` (wire), `mr_` (mapred), `mk_` (merkle), `db_` (the composed
database), and `lk_` (the definition-lookup tool). Read any of them with
`System.Open <name>` in the GUI or `latte eval --lib name=lib/name.lat "…"` from
the shell.
