# Data-Intensive Techniques — an interview guide

This manual works through the core techniques from Martin Kleppmann's *Designing
Data-Intensive Applications* (DDIA), each implemented as a small **Latte** library
in `lib/`. The goal is interview readiness: for every technique you get the idea
in one paragraph, the data structure, a runnable example, the trade-offs, and the
questions an interviewer actually asks — answered.

It closes with a tour of how **Orpheus itself** is a data-intensive system, so the
algorithms here are not toys bolted on the side: the host's event log, gossip,
logical clocks, and convergent state are the same ideas running for real.

**New to data systems?** If terms like *LSM-tree*, *replication*, or *consensus* are
unfamiliar, read the **beginner's on-ramp** just below ("New to data systems? Start
here") before Part I — it explains why this whole field exists, what each part answers
in plain language, and the vocabulary, in about ten minutes.

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

---

# New to data systems? Start here

This section is the on-ramp. The parts that follow are written for someone preparing for
interviews, so they move quickly and use the field's vocabulary freely. Read this first and
that vocabulary will already make sense. No prior database or distributed-systems experience is
assumed — only patience and a willingness to run the examples.

## Why this whole field exists

Picture one program, on one computer, with one disk. It stores data and hands it back. For a
long time that is enough. Then one of three pressures arrives, and *every* technique in this
guide is an answer to one of them:

- **The data outgrows one disk.** You have more data than a single machine can hold, so you must
  spread it across many machines (*Partitioning*) and store it cleverly on each (*Storage and
  retrieval*).
- **The traffic outgrows one machine.** More users are reading and writing than one machine can
  serve, so you keep copies on several machines to share the load (*Replication*) — and now those
  copies can disagree.
- **Machines fail.** Disks die, networks drop, programs crash mid-write. A serious system must
  keep working and keep its data correct anyway (*Transactions*, *Replication*, *Consensus*).

"Data-intensive" means the hard part is the **data** — its size, its rate of change, the need to
keep it correct across failures — not raw arithmetic. That is the opposite of "compute-intensive"
work like rendering or simulation.

## The one mindset: there is no free lunch

The single most useful habit when studying this material is to ask, for every design, *what did
it give up to get what it gained?* Faster writes usually cost slower reads. Keeping copies in
sync instantly costs availability when the network breaks. More safety costs more coordination.
The interviews in this guide are mostly about naming these trade-offs out loud, so train the
reflex now: every technique buys something and pays for it somewhere.

## A map of the guide, in plain language

Each part answers one plain question. Skim this, then dive in:

- **Part I — Storage and retrieval.** Where do I put a write, and how do I find it again
  quickly? (B-trees, LSM-trees, Bloom filters.)
- **Part II — Encoding and evolution.** How do I save data to bytes so that a newer or older
  version of the program can still read it?
- **Part III — Replication.** I keep copies on several machines so one death does not lose data —
  how do I keep the copies in step, and what happens when they disagree?
- **Part IV — Partitioning.** The data is too big for one machine, so I split it across many —
  which machine holds which key, and how do I add machines without reshuffling everything?
- **Part V — Transactions and isolation.** Many people change the data at the same time — how do
  I stop their changes from corrupting each other?
- **Part VI — Time, order, and consensus.** Separate machines have no shared clock — how do they
  agree on what happened first, and how do they agree on a single decision even when some fail?
- **Parts VII–VIII — Batch and stream processing.** How do I process an enormous pile of data —
  all at once (batch), or piece by piece as it streams in?
- **Part IX — Probabilistic structures.** How do I count or measure a gigantic stream
  *approximately* but in tiny memory (how many unique visitors? which items are hot)?
- **Part IX½ — A composed database.** All of the above, assembled into one small but real working
  database you can read end to end.

## Keep this glossary handy

These words recur everywhere below. Plain-language definitions:

- **index** — a side structure that lets you find data fast without scanning everything, like a
  book's index.
- **B-tree** — the classic index that updates data *in place*; great for reads. **LSM-tree** —
  an index that only ever *appends* new data and tidies up later; great for writes.
- **memtable / SSTable** — in an LSM-tree, the small in-memory buffer of recent writes
  (memtable) and the sorted on-disk files it is flushed into (SSTables).
- **tombstone** — a little marker that records "this key was deleted" (you cannot erase from an
  append-only file, so you append a deletion note).
- **compaction** — the background tidy-up that merges those files and discards overwritten and
  deleted entries.
- **read / write amplification** — doing more work under the hood than the request implies: a
  read touching many files, or a byte being rewritten many times during compaction.
- **replica** — one of several copies of the data on different machines. **partition / shard** —
  one slice of the data when it is split across machines.
- **consistency** — how up-to-date and in-agreement the copies are. **eventual consistency** —
  copies may lag briefly but converge if writes stop.
- **transaction** — a group of changes that must all happen or none happen. **isolation** —
  keeping concurrent transactions from seeing each other's half-finished work.
- **MVCC / snapshot** — keeping multiple versions of each value so a reader sees a consistent
  *snapshot* of the data as of a moment, without blocking writers.
- **idempotent** — safe to apply more than once with the same effect (important when a message
  might be delivered twice).
- **quorum** — requiring a majority of replicas to agree on a read or write, so reads and writes
  overlap on at least one up-to-date copy.
- **vector clock** — a per-machine tally that detects whether two updates are ordered or are
  genuine concurrent conflicts. **CRDT** — a data type designed so concurrent edits *always*
  merge cleanly, no conflict possible.
- **consensus** — getting separate machines to agree on a single value or order even when some
  crash (Raft is the algorithm shown here). **leader** — the one replica others follow.
- **log / WAL** — an append-only record of every change; a *write-ahead log* is written before
  the change is applied, so a crash can be recovered by replaying it.
- **hash function** — a function turning data into a fixed-size number, used to place keys,
  test membership (Bloom filters), and estimate counts (HyperLogLog).

## How to study this guide

A method that works well here:

1. **Read the one-paragraph idea** at the top of a section first — what problem it solves —
   before looking at any code.
2. **Run the sample one-liner** (every example ships a `<name>_sample` fixture) and read what
   comes back, matching it to the idea.
3. **Read the "In an interview" notes** — that is where the trade-offs live, and the trade-offs
   are the real content.
4. **Return to the glossary** whenever a word feels fuzzy; the words are reused part to part.

Start with Part I; the later parts lean on its vocabulary. Take one section at a time — this is a
guide to *study*, not to skim.

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
(bt_min (bt_sample 0))               :: smallest entry
(bt_max (bt_sample 0))               :: largest entry
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
`g_i = h1 + i·h2`, so two base hashes give `k` independent-looking probes. The base
hashes are polynomial (Horner) hashes finished with a multiplicative avalanche step,
and ranges are taken from the high bits — see `lib/dhash.lat`. The packed bit array
itself is manipulated through the bitwise operators now in `std`: a bit is set by
OR-ing in `1 << i` (`bor n (shl 1 i)`), tested with `bit n i`, and the filter's fill
is `popcount`. (Bitwise XOR is now available too, so an FNV-style hash is expressible;
the polynomial hash is kept for its proven dispersion.)

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
:: the same anomaly in db.lat, run BOTH ways (doctors-on-call):
(db_skew_si 0)    :: %commit — snapshot isolation lets the second writer through (the bug)
(db_skew_ser 0)   :: %abort  — serializable read-set validation prevents it
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
  (predicate/range locks — correct but contended); or **optimistic concurrency
  control on a snapshot**, which runs without locks and aborts at commit if the world
  it read has changed. `mvcc_ssi_try` (and `db_commit` in `db.lat`) implement that
  optimistic route: alongside the write-write check they **validate the read set** —
  if any key the transaction read was overwritten by a commit during its lifetime,
  its decision rested on stale data, so it aborts. That is exactly what turns the
  write-skew example from a commit into an abort.
- *Is read-set validation the same as SSI?* Not quite, and it is worth being precise:
  what is implemented here is plain **optimistic concurrency control** — revalidate
  every read at commit. True **serializable snapshot isolation** (Cahill et al. 2008,
  PostgreSQL's `SERIALIZABLE`) is cleverer: it tracks read-write *dependency edges*
  between live transactions and aborts only when they form the specific dangerous
  structure that makes a schedule non-serializable, so it accepts many schedules that
  blanket read-set validation would needlessly reject. OCC is the correct, simpler,
  more conservative cousin: same guarantee, more false aborts. `db.lat` shows the
  contrast directly — `db_commit_si` is snapshot isolation (write-write checks only,
  so `db_skew_si` lets the doctors-on-call anomaly through) and `db_commit` adds
  read-set validation (`db_skew_ser` aborts the second transaction and prevents it).
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
engine deep enough to show the techniques the way DDIA presents them — it is the
through-line of Parts I–II in a single module. A `db` value is the tuple
`[ store index bloom idxtag rschema ts wal ]`, and each layer is a chapter:

> **There is an interactive tour of all of this.** Open `System.OpenText
> database-tutorial` (or the **Interactive tutorial** button in the Db tool): a text
> whose every framed panel is a live object run against the real database — point
> reads, the Bloom filter answering "absent", a functional write, an index query,
> MVCC history, the write-ahead log, and the findb application — with buttons and
> fields so you can poke it while you read. Tool fences in any text now rehydrate
> through the full command set, so `db.*` and package arms embed live on load.
>
> It is also fast now. A `db_put` runs the whole stack — log, Bloom add, index,
> version chain — and the Bloom add dominated, because setting a bit in the packed
> bitset (now `bor n (shl 1 i)`) calls `pow 2 i` (a 128-byte big-integer) built by
> *naive* repeated multiplication. Making `std.pow` binary-exponentiation (O(log e)) cut a 24-bar
> load from ~0.5s to ~0.1s and the findb dashboard from seconds to a fifth of a
> second — and it speeds up cord building too, since `cat` shifts by `pow 256 len`.


| Layer | Library | DDIA |
|---|---|---|
| primary key → **version chain** | LSM-tree (`lsm`) | Ch. 3 (storage) + Ch. 7 (MVCC) |
| every mutation logged before it applies | the `wal` field | Ch. 3 (durability / recovery) |
| "definitely absent?" before any read | Bloom filter (`bloom`) | Ch. 3 (negative lookups) |
| indexed field value → primary keys | a second LSM (`lsm`) | Ch. 3 (secondary index) |
| tagged records, evolved on read | `wire` | Ch. 4 (encoding/evolution) |
| key → shard | consistent hashing (`chash`) | Ch. 6 (partitioning) |
| snapshot reads + read-set validation | the version chains | Ch. 7 (snapshot isolation; OCC for serializable) |
| N replicas, quorum W/R, siblings | `quorum` + `crdt` + `vclock` | Ch. 5 (leaderless replication) |
| atomic commit across shards | `tpc_*` | Ch. 9 (two-phase commit) |

### Storage, durability, and MVCC

The primary store maps each key to a **version chain**: a list of `[ts value]`
pairs, newest first, where a value is a record or the tombstone `%tomb`. A write
prepends a new version; nothing is overwritten. That single representation buys
three DDIA ideas at once:

- **Snapshot isolation (Ch.7).** A reader at snapshot timestamp `T` sees the newest
  version with `ts <= T` — a consistent point in time, with *time-travel* for free.
- **Durability and crash recovery (Ch.3).** Every put/delete is appended to a
  write-ahead log *before* it touches the store, so the log is the source of truth
  and the in-memory state is a cache. `db_recover` replays the log onto a fresh db
  and reconstructs every chain, the index, the Bloom filter, and the clock.
- **Deletes that respect history.** A delete appends a `%tomb` version rather than
  erasing, so an old snapshot still sees the row while new reads see absence.

```
(db_get (db_sample 0) %u1)                       :: -> [%rec [[1 %alice] [[2 %nyc] 0]]]
(db_get (db_sample 0) %u9)                        :: %absent — Bloom skipped the store
:: MVCC time-travel: write a new version, then read an OLD snapshot
(db_getat (db_put (db_sample 0) %u1 [[1 %alice2] 0]) %u1 1)   :: -> the original alice
(db_history (db_put (db_sample 0) %u1 [[1 %alice2] 0]) %u1)   :: -> a 2-version chain
:: crash recovery: the write-ahead log replays onto an empty db
(db_get (db_recover (db_open 2 0 4) (db_wal (db_sample 0))) %u3)  :: -> carol, rebuilt from the log
(len (db_wal (db_sample 0)))                      :: 3 logged mutations
```

### Persistence — durable, named databases that survive restarts

`db_recover` shows the engine *can* rebuild itself from a log; persistence makes
that log live on disk. The `db_*` arms are pure Latte, so a database value lasts one
evaluation and is then gone — exactly the right default for a value, and the reason
the in-language model never touches the filesystem. Durability is added *around*
that core, by a small host service (`src/dbservice.rs`), not by weakening it:

- each write is appended to an on-disk write-ahead log (`<name>.wal`) and flushed
  **before** the operation is reported as done;
- on open, the log is replayed through the real `db_put`/`db_delete` arms to
  rebuild the in-memory value — the same `db_recover` idea, now reading from disk;
- the live value is held as a noun between requests and threaded through the Latte
  arms via `call_arm`, so all the database logic stays in `db.lat`; only persistence
  and the cross-request lifetime are in Rust.

The result is a genuinely persistent, named, multi-version database, reachable three
ways — the **Data** tool in the GUI, the `/api/db` endpoint, and the command line:

```
latte db people put u1 '[ [1 %alice] [ [2 %nyc] 0 ] ]'   # logged to ./dbdata/people.wal
latte db people put u2 '[ [1 %bob]   [ [2 %sfo] 0 ] ]'
latte db people get u1            # a DIFFERENT process — replays the log, prints alice
latte db people put u1 '[ [1 %alice2] [ [2 %la] 0 ] ]'   # a new MVCC version
latte db people history u1        # both versions, newest first — survived the restart
latte db people query nyc         # the secondary index, rebuilt on replay
latte db people dash              # the whole live state as HTML
latte db sales  agg 1 2           # GROUP BY field 1, SUM field 2 — analytics over on-disk data
latte db people checkpoint        # compact the log to one entry per live key
latte db bank   txn "a=[ [1 %usd] [ [2 70] 0 ] ] | b=[ [1 %usd] [ [2 30] 0 ] ]"  # atomic multi-key write
```

`db.lat` already implements serializable transactions as pure values (snapshot reads, optimistic
concurrency control, write-skew prevention — the doctors-on-call demo); the durable service exposes
them. `begin` returns a snapshot timestamp; `txn` takes that snapshot, the keys the caller read, and
the writes to apply. It runs `db_commit`, which validates the read-set (aborting if any read key has
a committed version newer than the snapshot, which is what closes write skew) and then applies every
write or none. On commit the whole write-set is appended to the log as one crash-atomic batch — a
`T<n>` framing line followed by its n `P` lines, written and flushed together — and on recovery a
batch with fewer than n lines following it (a write torn by a crash) is dropped whole, so a
transaction is all-or-nothing across a restart. The CLI `txn` form (empty read-set) is an atomic,
durable multi-key write; the read-set validation that makes it serializable is exercised by the
service tests.

Because the log is appended to forever, a long-lived database would replay its whole history on
every open. `checkpoint` (alias `compact`) bounds that: it rewrites the log as a single `P` entry
per live key holding that key's current value, dropping superseded versions and deleted keys, then
rebuilds the live value from the compacted log so memory and disk agree. The current state is
preserved exactly (each record's noun is re-serialized to a Latte literal that re-evaluates to the
same noun) and survives a restart, but the tradeoff is the standard one for a checkpoint: the
per-version MVCC history *before* the checkpoint is collapsed to the current value; history
accumulates again afterwards. The rewrite is installed atomically (temp file, flush, rename), so a
crash mid-checkpoint leaves the original log intact.

On top of the storage engine, `db.lat` carries a small **relational-style query/analytics layer**
that runs as compiled Latte: `db_scan`/`db_where`/`db_pluck` (scan, filter, project),
`db_count`/`db_sum`/`db_min`/`db_max`/`db_avg` (aggregates over a field), `db_order`/`db_topn`
(sort and top-N), and `db_groups`/`db_groupsum`/`db_groupavg` (GROUP BY). Because `db_pluck`
projects a queried column into a plain list — and `db_npluck` into signed fixed-point — a query
result drops straight into the statistics/ML library (`st_mean`, `st_std`, regression) and the
plotter (`bars`, `points`), so the database is the front of a single pipeline: **stored records →
query/aggregate → train/visualize**, all native. This generalizes the bespoke pipeline in
`findb.lat` (financial bars stored in the db, read back for a regression fit and a sparkline) to any
database and any field.

The same layer carries an **index-aware query planner** and relational operators. `db_select`
runs an equality predicate through the secondary index when the field is the indexed one (a
posting-list probe) and otherwise falls back to a scan-and-filter — the database picks the plan.
`db_idxrange` is a true non-primary-key range scan over the indexed field; because the index is
ordered in the dhash byte/cord collation, it is index-accelerated and correct for cord-valued
fields (names, cities, regions), while numeric fields — which don't sort byte-wise — are ranged
with `db_between` (a scan plus a numeric filter, the honest cost of ranging a field the index
doesn't order). `db_and`/`db_or` compose result sets by primary key, and `db_join` is a
nested-loop equi-join of two result sets on a chosen field from each. All of it compiles to native.

Because the log is the source of truth and every layer (chains, index, Bloom filter,
timestamp counter) is derived from it on replay, MVCC history and index queries come
back intact after a restart — verified by the `dbservice` tests, which write in one
service instance and read from a second over the same directory.

### Indexing, encoding, partitioning

The Bloom filter shortcuts a guaranteed miss so a lookup for an absent key touches
no run at all. The secondary index keeps a posting list of primary keys per field
value, and `db_reindex` moves a key between posting lists when its indexed field
changes — so the index stays correct under updates and deletes, not just inserts.
Records are wire field lists read through a reader schema (schema evolution), and a
consistent-hash ring routes a key to its shard.

```
(map (fn [r] -> (head r)) (db_query (db_sample 0) %nyc))   :: secondary index -> [%u3 %u1]
:: update u1's city nyc -> sfo; the index follows the change
(map (fn [r] -> (head r)) (db_query (db_put (db_sample 0) %u1 [[1 %alice] [[2 %sfo] 0]]) %nyc))  :: -> [%u3]
(map (fn [e] -> (head e)) (db_range (db_sample 0) %u1 %u2))    :: primary range scan -> [%u1 %u2]
(db_route (ch_build [%n1 [%n2 [%n3 0]]] 16 65536) %u1 65536)   :: which shard owns u1
:: schema evolution: a row with no field 3, read through a schema that supplies its default
(db_get (db_put (db_open 2 [[1 0] [[2 0] [[3 %zz] 0]]] 4) %u1 [[1 %alice] [[2 %nyc] 0]]) %u1)
  :: -> [%rec [[1 %alice] [[2 %nyc] [[3 %zz] 0]]]]
```

A secondary index can be **local** (document-partitioned: each shard indexes its own
rows, and a field query must scatter/gather across all shards) or **global**
(term-partitioned: the index itself is partitioned by field value, so a field query
routes to one shard but a write must update a remote index partition). `db.lat`'s
index is local; the routing function shows where a global partition would live.

### Serializable transactions

`db_begin` takes the current timestamp as a read snapshot (free — a `db` value is
immutable). A transaction reads through that snapshot with `db_txget` and, at commit,
is validated: if any key it *read* has a committed version newer than the snapshot,
it aborts (first-committer-wins). That read-set check is what refuses write skew.
It is the validation mechanism at the heart of serializable snapshot isolation,
applied to the read set; it does not track the full graph of rw-antidependencies a
production SSI scheduler would, so it can abort some schedules that were in fact
serializable — safe, but conservative.

```
:: a snapshot read is stable: a write committed after begin is invisible to it
(let d = (db_sample 0) in let s = (db_begin d) in (db_txget (db_put d %u1 [[1 %changed] 0]) s %u1))
  :: -> [%rec [[1 %alice] ..]]  (still the snapshot's value)
:: a transaction whose read set still holds commits...
(head (db_commit (db_sample 0) (db_begin (db_sample 0)) [%u1 0] [[%u4 [[1 %dave] 0]] 0]))  :: %commit
:: ...but if u1 changed after the snapshot, the transaction that read it aborts
(let snap = (db_sample 0) in let cur = (db_put snap %u1 [[1 %alx] 0]) in
  (head (db_commit cur (db_begin snap) [%u1 0] [[%u2 [[1 %bz] 0]] 0])))                     :: %abort
```

### Leaderless replication (Dynamo-style)

`clu_*` builds a cluster of N replicas with quorum parameters W and R. Each replica
holds, per key, an **MV-Register** CRDT — a set of causally-maximal `[value, vector
clock]` entries. A write stamps a vector clock and lands on W replicas; a read
merges the registers from R replicas. When `W + R > N` the write and read sets
overlap, so a read is guaranteed to see the latest write. Concurrent writes that do
not descend from one another survive as **siblings** for the application to resolve;
a later coordinated write supersedes them. **Read repair** writes the merged result
back across the read set, healing stale replicas.

```
(clu_strong (clu_sample 0))                               :: 0 = W+R>N, reads see latest
(clu_get (clu_put (clu_sample 0) %k %v1 %n0) %k)          :: -> [%val %v1]
:: two blind concurrent writes from different nodes -> siblings
(clu_get (clu_putblind (clu_putblind (clu_sample 0) %k %va %n0) %k %vb %n1) %k)  :: -> [%siblings [%vb %va]]
:: a coordinated read-modify-write descends from both and collapses them
(clu_get (clu_put (clu_putblind (clu_putblind (clu_sample 0) %k %va %n0) %k %vb %n1) %k %vc %n0) %k)  :: -> [%val %vc]
```

### Atomic commit across shards

When a transaction spans partitions, `tpc_*` runs two-phase commit: every
participant votes (`%yes`/`%no`) and the coordinator commits iff the vote is
unanimous. Like the raft module it is the safety skeleton — the uniform-decision
guarantee — without networking, timeouts, or a coordinator-recovery log.

```
(head (tpc_run [%sA [%sB [%sC 0]]] (fn [s] -> %yes)))                              :: %commit
(head (tpc_run [%sA [%sB [%sC 0]]] (fn [s] -> if (s == %sB) then %no else %yes)))  :: %abort
```

Run the whole tour with `latte ddia db`.

**In an interview**
- *Why version chains?* They unify three things a database needs anyway: snapshot
  isolation (read at a timestamp), MVCC reads without read locks, and a natural
  place for tombstones. Snapshots are free because a `db` value is immutable.
- *Why is W+R>N enough?* It forces the write quorum and the read quorum to share at
  least one replica, so a read always contacts someone who saw the latest write. It
  does not by itself resolve *concurrent* writes — that is what the vector clocks and
  siblings are for.
- *Where would this differ from production?* Honest simplifications: the whole
  version chain is stored as one LSM value rather than one entry per version, so
  `db_compact` merges LSM segments but does **not** garbage-collect old versions —
  a real MVCC engine drops versions no open snapshot can still see, while here a
  chain grows for the life of the key; the Bloom filter cannot un-set a deleted key
  (a tunable false positive, never a wrong answer); the secondary index is local
  (the global/term-partitioned variant is described but not run across shards); and
  2PC is the safety core without failure handling, like raft. The *reasoning* in
  each layer is the real thing.

This module also exercises three small Latte features added for it: short-circuit
`and`/`or` (lazy in the second operand, so a guard never evaluates an unsafe right
side), multi-binding `let` (`let a = x, b = (f a) in …`), and the `aget`/`aput`/
`akeys` association-list helpers and `nub` in `std` (see `latte-language.md`).

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
- **Definition lookup (`System.Def`)** — select a function name anywhere in the
  GUI (in a code frame, **double-click the name** to select the word, or drag to
  highlight it) and click **≡ Def** (or run `System.Def`, or `System.Def <name>`);
  the host runs the `lookup` library
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

## The text-frame front end (Db.Tool)

In the System GUI, the **Db** header link opens `Db.Tool` — an editable command
sheet over the composed database. Its buttons run query arms that return tagged
HTML (`[%html cord]`) and embed a live view wherever you click:

- **[Show the database state]** runs `db.db_dash0`, whose arm `db_dash` scans the
  key range with `db_range` and tabulates each key's name, city, and MVCC version
  count, with the write-ahead-log length beneath.
- An **add-a-row** control has `key`, `name`, and `city` fields feeding
  `db.db_dash (db_put (db_sample 0) $key [[1 $name] [[2 $city] 0]]) %u0 %u9` — a
  functional update of the (immutable) sample, re-rendered.
- A **query** control feeds `db.db_queryhtml (db_sample 0) $qcity`, listing every
  live row whose indexed city equals the field (the secondary index).
- A **time-travel** control feeds `db.db_historyhtml …`, showing the full MVCC
  version chain of one key, newest first (a tombstone reads "(deleted)").

The render arms are in `lib/db.lat`: `db_dash` / `db_dash0`, `db_queryhtml`, and
`db_historyhtml`, with helpers `db_rec` (unwrap a `[pk [%rec fields]]` entry),
`db_trow` / `db_rows`, `db_queryli`, and `db_verli`. They build HTML cords with the
`std` toolkit (`catall`, `numtext`) and read fields through `db_field`. Edit a
field and click, edit a command line (records and ballots are typed inline because
`[field: …]` values cannot contain `]`) and middle-click it, or edit the arms and
**Compile**; **Store** persists your sheet to `text/db-tool.md`, overriding the
built-in one next run.

## The database, applied: a symbol index for code navigation

The composed database is not only a demo — it backs a real navigation tool for the
system's own code. `lib/symbols.lat` (`import db`) treats every arm of every loaded
library as a record `[ [1 name] [2 module] [3 arity] ]`, stored under the unique key
`module.name` and indexed on the name (field 1). Three questions become database
operations:

- **who defines `name`?** — a secondary-index query (`sy_defs` / `sy_modulesof`).
- **is `name` shadowed?** — that query returns more than one module (`sy_shadowed`).
  This matters because the whole system shares one flat scope: `transpose`, for
  instance, is defined in `plan`, `nn`, and `fin`, so the library loaded last wins
  and silently shadows the others. The index makes that visible.
- **how did `module.name` change?** — its MVCC version chain (`sy_history`). Because
  Compile re-registers a module's arms at runtime, re-indexing a redefined arm is
  just another versioned write the store already records (`sy_redefine`).

`sy_sample` builds a small index with a deliberate three-way collision on `dot`;
`latte ddia symbols` runs the queries. Try them in the shell:

    :: how many modules define dot, and which ones
    latte eval '(sy_count (sy_sample 0) %dot)'
    latte eval '(sy_modulesof (sy_sample 0) %dot)'
    :: 0 = shadowed (more than one definer); gross has a single definer
    latte eval '(sy_shadowed (sy_sample 0) %dot)'
    :: recompiling an arm appends an MVCC version
    latte eval '(sy_revisions (sy_redefine (sy_sample 0) %plan %dot 9) %plan %dot)'

### In the GUI: the Sym tool and a better System.Def

The **Sym** header link opens `Sym.Tool`: type a name and `sym <name>` embeds a live
table of every loaded module that defines it, with arities and a shadow warning. The
host (`/api/symbols`) hands the name's definers to `symbols.lat`, which indexes and
renders them; scoping each request to one name's definers keeps the immutable build
small and fast.

The same idea improves the existing **System.Def** (the header's ≡ Def, or
`System.Def <name>`): it used to print the first module that happened to define a
highlighted name. It now reports *every* module that defines it and which one wins by
load order — so a name resolving to an unexpected definition is no longer a mystery:

    transpose is defined in 3 modules: plan, nn, fin
    fin wins (loaded last); the rest are shadowed in the shared scope.

    module fin

      transpose = fn [rows] -> …

## The database, applied: financial data for visualization and training — `lib/findb.lat`

The symbol index stores *code* in the database; `findb.lat` stores *data* in it —
a window of financial price bars — and then puts the stored data to work twice:
once for visualization, once for training a model. It is built on `lib/db.lat`
(the composed database) and `lib/stats.lat` (the statistics library), and every
number it reports is read back out of the store, not kept on the side.

A bar is a record `[ [1 day] [2 close] [3 dir] ]` keyed `d<i>` and indexed on the
day field, so each posting list holds one bar and loading a window stays cheap:

    :: load a list of closes into the store, then read day 3 back out
    latte eval '(fd_close (fd_load (fd_prices 0)) 3)'
    :: the whole window, read one point at a time from the database
    latte eval '(fd_closes (fd_sample 0) 12)'

**Visualization.** `fd_spark` reads the stored closes and returns an `[%svg …]`
sparkline; the GUI embeds it live. The data path is the point: the polyline is
drawn from what the database returns, not from a Rust array.

    latte eval '(fd_spark (fd_sample 0) 12)'      :: -> [%svg <polyline …>]

**Training.** `fd_fit` reads the window, turns it into daily returns, and fits a
lag-1 least-squares line — tomorrow's return regressed on today's — returning
`[slope intercept correlation]`. A negative slope is mean reversion; the
correlation is its strength. This is a (small) model trained entirely on data
pulled from the store, with the regression supplied by `stats.lat`:

    latte eval '(fd_fit_of (fd_prices 0))'        :: -> [slope intercept corr]
    latte eval '(fd_forecast_of (fd_prices 0))'   :: next-day return prediction

**Corrections are versioned.** Re-storing a bar is an MVCC write, so a fix keeps
the prior reading in history — the audit trail the database already gives every
key:

    :: correct day 0, then count the versions on that key (= 2)
    latte eval '(len (fd_history (fd_correct (fd_sample 0) 0 (npos 101000) 0) 0))'

`fd_dash` assembles the sparkline, the summary statistics (mean, standard
deviation, realized volatility, up-moves), the fitted model, and the next-day
forecast into one `[%html …]` dashboard. The **Findb** header link and the
`findb market=… n=…` command run it over a live market window: the host fetches
the last *n* closes, loads them into the store, and renders what it reads back
(`/api/findb`). One honest limit: the immutable store is built in the
interpreter, so a window is tens of bars — a dashboard's worth — while the full
multi-year series stays in the Rust path behind the Chart tool. Assembling the
page would have been quadratic, too, because cords are big-integers and a
left-folded concatenation re-shifts the whole accumulator each step; `std.catbal`
pairs cords and halves the list instead, so the dashboard builds in O(n log n).
