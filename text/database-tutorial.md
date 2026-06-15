# The composed database — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable. This particular tutorial
builds on data-systems ideas (LSM-trees, write-ahead logs, MVCC); if those words are new, the
**data-intensive guide** opens with a plain-language on-ramp that explains them
(`System.Edit data-intensive`).

This is a working tutorial, not a screenshot. Every framed panel below is a **live
object**: when this text loads, the system runs a real command against the real
database in `lib/db.lat` and embeds what comes back. The buttons run more commands
in place, and the input fields feed them — so you can poke the database while you
read about it. Edit any command line and middle-click it to re-run.

The database is *composed* from the storage-engine ideas in Kleppmann's **Designing
Data-Intensive Applications**, each its own small Latte module: a log-structured
store (`lsm`), a write-ahead log for durability, a **Bloom filter** to skip reads,
a **secondary index** for non-key lookups, and **MVCC** version chains for
snapshots and time-travel. `db.lat` wires them into one value you thread through
pure functions — there is no hidden mutable state.

## 1. The sample: three users, keyed and indexed

`db_sample 0` builds a tiny database of three users, keyed `u1`–`u3` and **indexed
on their city** (record field 2). Here is its whole state — the primary store on
the left idea, the secondary index (city → keys) on the right, and the write-ahead
log count at the foot:

```tool db.db_dash0
```

Each row is a record `[ [1 name] [2 city] ]`. The key (`u1`) is separate from the
fields; the index lets you find rows by city without scanning every key.

## 2. Reading a key — a point read

`db_get` looks a key up and returns its **currently-visible version**. The shape it
returns is `[%rec field-list]` — the `%rec` tag marks a present record:

```tool eval (db_get (db_sample 0) %u1)
```

Try another key (the sample has `%u1`, `%u2`, `%u3`):

key [field: rk=%u2]
[Read that key](run: eval (db_get (db_sample 0) $rk))

## 3. The Bloom filter — answering "absent" without a read

Before touching the store, a read asks a **Bloom filter**: *could* this key be
here? The filter never says "no" about a key that is present (no false negatives),
so a "definitely absent" answer lets the engine skip the store entirely. Ask for a
key that was never inserted and the database reports `%absent` — the Bloom filter
short-circuited the lookup:

```tool eval (db_get (db_sample 0) %u9)
```

The filter is `m` bits set by `k` hashes of the key (Kirsch–Mitzenmacher double
hashing). A "possibly present" answer can be a false positive — at worst one wasted
store read — but never a missed key. (Setting those bits used to be the slowest
part of a write; it is now `pow`-by-squaring fast.)

## 4. Writing is functional — the old database still exists

`db_put` does **not** mutate anything. It returns a *new* database value that shares
almost all of its structure with the old one. Watch a fourth user appear when we
add `u4` and show the result — yet `db_sample 0` itself is unchanged, because the
put produced a separate value:

```tool db.db_dash (db_put (db_sample 0) %u4 [ [1 %dave] [ [2 %nyc] 0 ] ]) %u0 %u9
```

Add your own row, then show the state it produces:

key [field: nk=%u5]  name [field: nn=%erin]  city [field: nc=%la]
[Add row & show](run: db.db_dash (db_put (db_sample 0) $nk [ [1 $nn] [ [2 $nc] 0 ] ]) %u0 %u9)

## 5. The secondary index — find rows by a non-key field

Because the sample is indexed on city, "who lives in NYC?" is an **index lookup**,
not a full scan: the index maps a city to the list of keys, and each key is then a
point read. Query the index by city:

```tool db.db_queryhtml (db_sample 0) %nyc
```

Type a different city and ask again (the sample uses `%nyc`, `%la`, `%sf`):

city [field: qc=%la]
[Query by city](run: db.db_queryhtml (db_sample 0) $qc)

A write keeps the index in step automatically: `db_put` of a row whose city changed
removes the key from the old city's list and adds it to the new one.

## 6. MVCC and time-travel — old versions stay readable

Every write appends a **new version** to the key's chain, stamped with a logical
timestamp; it does not overwrite the old one. So moving `u1` to a new city leaves
the previous reading intact, and the full history is still there to inspect
(newest first):

```tool db.db_historyhtml (db_put (db_sample 0) %u1 [ [1 %alice2] [ [2 %la] 0 ] ]) %u1
```

This is what makes **snapshot isolation** possible: a reader pins a timestamp and
sees a consistent database as of that moment, even while writers append newer
versions. `db_getat d key ts` reads a key *as of* a past timestamp — a query
against the past, for free, because the past was never thrown away.

## 7. Isolation levels — and the anomaly snapshot isolation misses

Snapshot isolation is wonderful, but it is **not** serializable, and the gap has a
name: **write skew**. The textbook example is two on-call doctors. The rule is "at
least one must stay on call." Both are on. Two transactions run at the same instant;
each reads the schedule, sees the *other* doctor still on, and takes *itself* off.
Their writes touch *different* rows, so neither conflicts with the other — and both
commit. Now nobody is on call. Each transaction was correct in isolation; together
they broke the invariant, because each reasoned about a row the other then changed.

`db.lat` runs exactly this scenario two ways. Under **snapshot isolation**
(`db_commit_si`, which only checks for write-*write* conflicts), the second
transaction commits — the anomaly happens:

```tool eval (db_skew_si 0)
```

That `%commit` is the bug. To get **serializability**, the commit must also check the
*reads*: optimistic concurrency control re-validates the read set, and since the
second transaction read a doctor the first one changed, it aborts:

```tool eval (db_skew_ser 0)
```

`%abort` — the anomaly is prevented, at the cost of retrying the loser. That extra
read-set check is the whole difference between "snapshot isolation" and
"serializable" here. (Production databases like PostgreSQL use a sharper version,
*serializable snapshot isolation*, that aborts less often by tracking which
read-write dependencies are genuinely dangerous; the version above is its simpler,
more conservative cousin — same guarantee, a few more needless aborts.)

## 8. Durability — the write-ahead log

Each write is appended to a **write-ahead log** *before* the in-memory state is
updated, so the log is the source of truth and the live structures are a cache it
can rebuild. The sample's log holds one entry per write so far:

```tool eval (len (db_wal (db_sample 0)))
```

After a crash, `db_recover` replays that log onto a fresh, empty database and
reconstructs every chain, the index, the Bloom filter, and the timestamp counter —
so the recovered database reads identically to the original. The log *is* the
database; everything else is derived from it.

This is also how the database is made **persistent**. The `db_*` arms above are pure
Latte: a value lasts one evaluation and is then gone. The **Data** tool (and `latte
db …` on the command line) wraps exactly this engine in a host service that writes
each operation to an on-disk write-ahead log and replays it on open — so named
databases survive restarts. Store a row, close everything, reopen tomorrow: still
there. Same LSM, Bloom filter, secondary index, and MVCC history — now durable.

[Write a row to disk](run: data demo put alice [ [1 %alice] [ [2 %nyc] 0 ] ]) [Show the persistent database](run: data demo dash)

Those buttons actually touch the disk. Open the **Data** tool to add more and inspect
version history — or run `latte db demo dash` in a shell. Reopen any time; it persists.

## 9. From rows to prices — the same database, applied

Nothing about this store is user-specific. `lib/findb.lat` keeps a window of
financial price bars in exactly this database — one record per day — then reads the
window back out to draw a sparkline and to **train a small model** (a lag-1
least-squares fit of tomorrow's return on today's). Visualization and training,
both fed from the database:

```tool findb.fd_dash (fd_sample 0) 12
```

The summary statistics there come from `lib/stats.lat`; the sparkline and the fit
are computed entirely from what the store returns. Loading and rendering this used
to take seconds — the same `pow`-by-squaring and balanced-concatenation fixes that
sped up the Bloom filter brought it under a fifth of a second.

## Where to look next

- **`lib/db.lat`** — the composition: open `System.Open db` to read it, edit an arm,
  press Compile, and the change is live in the panels above.
- **The Data tool / `latte db`** — the persistent wrapper (`src/dbservice.rs`): named
  databases on an on-disk write-ahead log that survive restarts.
- **`latte ddia db`** (and `lsm`, `bloom`, `mvcc`, `findb`) — each technique as a
  command-line demo.
- **`docs/data-intensive.md`** — the full written guide, including the symbol index
  and the findb application.

> The whole database is a few hundred lines of Latte. You just ran it.
