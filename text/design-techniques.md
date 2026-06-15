# Data-structure design — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/design.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

"Design a structure that supports *these* operations at *this* cost" is its own interview
genre. Unlike an algorithm question, the work is not finding a clever procedure — it is
choosing a **representation** so that each required operation falls out cheaply. The recurring
move is *augmentation*: store a little extra alongside the data so the expensive query
becomes a lookup. Because Latte values are immutable, each operation here returns a *new*
structure and the caller threads it forward; that does not change the algorithmic content,
only the plumbing.

## 1. Min-stack — getMin in O(1)

**The contract.** A stack with the usual `push`, `pop`, `top`, plus `getMin` that returns the
smallest value currently in the stack — and every operation must be O(1).

**Why the obvious idea fails.** Keeping a single "current minimum" field breaks on `pop`: if
you pop the current minimum, what is the new one? Recomputing it is an O(n) scan, so a single
field cannot survive removals.

**The insight.** Make the minimum *stack-structured* like the data itself. Store with each
value the minimum of the stack **at the instant that value was pushed**. That snapshot is
exactly the minimum that should be restored when everything above it is popped — so each
level carries its own era's answer, and `getMin` is just reading the top node's snapshot.

After pushing 5, 2, 7, 1, the top value is 1:

```tool eval (dz_mstop (dz_ms0 0))
```

…and the minimum is also 1 — read directly off the top node, no scan:

```tool eval (dz_msmin (dz_ms0 0))
```

The decisive test is what happens on `pop`. Removing the 1 must restore the minimum to 2 —
the snapshot stored when 2 was pushed (5 and 2 were on the stack then, minimum 2):

```tool eval (dz_msmin (dz_mspop (dz_ms0 0)))
```

Each node holds `[value minSoFar]`, so the extra space is O(n) and every operation stays
O(1). That space-for-time augmentation is the whole lesson.

## 2. Queue from two stacks — amortised O(1)

**The contract.** Build a FIFO queue (first-in-first-out) using only LIFO stacks (last-in-
first-out) as primitives.

**The insight.** One stack reverses the order of whatever you push; reversing *twice* gets you
back to the original order. So keep two stacks: an **in** stack that receives new arrivals
(newest on top) and an **out** stack that releases them. When `out` is empty and you need to
dequeue, pour the entire `in` stack onto `out` — the pouring reverses it, so the *oldest*
element ends up on top, exactly where a FIFO wants it.

Enqueue 1, 2, 3 and the front of the queue is 1, not 3:

```tool eval (dz_qpeek (dz_q0 0))
```

Dequeuing returns elements in arrival order:

```tool eval (head (dz_deq (dz_q0 0)))
```

**The cost argument.** A single dequeue can be expensive — the one that triggers a flip
touches every queued element. But each element is moved from `in` to `out` *exactly once* in
its lifetime, so across any sequence of `n` operations the total work is O(n): the
**amortised** cost per operation is O(1). This is the canonical example of why amortised
analysis matters — a worst-case-O(n) step can still give an O(1)-per-operation structure.

## 3. LRU cache — evict the least-recently-used

**The contract.** A fixed-capacity key→value cache. `get` and `put` are both fast; when a
`put` would exceed capacity, discard the entry that was used least recently.

**The insight.** Two things must be cheap at once: looking a key up *by value*, and finding
the *least-recently-used* entry to evict. The textbook O(1) solution pairs a hash map (for
lookup) with a doubly-linked list ordered by recency (for eviction) and splices a node to the
front on every touch. Here we keep the recency list as a plain list — most-recently-used at
the front — which makes the *policy* perfectly legible even though the list operations are not
O(1).

The defining behaviour is eviction. Put `a`, `b`, `c` into a capacity-2 cache and `a` — the
oldest — is gone, leaving `c` and `b` newest-first:

```tool eval (dz_lrukeys (dz_lru0 0))
```

So a lookup of `a` misses (returns the 0 sentinel):

```tool eval (dz_lruget (dz_lru0 0) %a)
```

**The subtlety that separates a correct LRU from a fake one is that a *read* counts as use.**
If we `touch` `b` (what a real `get` does internally) and *then* insert `d`, the victim is
`c` — because touching `b` made `c` the least-recently-used. The cache keeps `d` and `b`:

```tool eval (dz_lrukeys (dz_lruput (dz_lrutouch (dz_lru0 0) %b) %d 4))
```

Had `get` *not* updated recency, `b` would have been evicted instead — a classic bug. The
recency reorder on every access is the heart of the structure, not an afterthought.

---

Design joins the rest of the toolkit — paradigms, data-structure patterns, weighted graphs,
number theory, bit manipulation, string algorithms, and grids. Every arm here is in
`lib/design.lat` (prefix `dz_`); operations return the new structure, and `latte design
<topic>` prints these in a terminal (topics: minstack, queue, lru).
