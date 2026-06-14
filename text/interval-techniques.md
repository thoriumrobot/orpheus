# Interval problems — an interactive tour

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/intervals.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Interval questions — "merge these ranges", "how many rooms do we need", "where do these
overlap" — look varied but almost all yield to a single preparatory move: **sort the
intervals, usually by start time.** Sorting turns a quadratic tangle of "does any pair
overlap?" into one left-to-right sweep where you only ever compare the interval in your hand to
the next one. Recognising that the sort is the hard part — and that the sweep after it is
linear — is the whole game.

Here an interval is the pair `[start end]`, and a list of them is a list of pairs.

## 1. Merging — collapsing overlaps

**Merge overlapping intervals** is the archetype. After sorting by start, sweep while holding
the current merged interval. Two cases, and only two: the next interval either *starts at or
before* the current end (they overlap — stretch the current end to cover both) or it starts
*later* (a gap — the current interval is finished, emit it and open a new one). Because the
list is sorted by start, you never have to look backward: any interval that could overlap the
current one must be the very next, which is why one pass suffices.

```tool eval (iv_merge (iv_sample0 0))
```

Note that the result is independent of input order — the function sorts first — and that
*touching* endpoints merge (`[1 2]` and `[2 3]` become `[1 3]`), a boundary convention worth
confirming aloud in an interview.

**Insert interval** is merge's more surgical cousin: given an *already sorted, disjoint* list,
slot in a new interval and absorb only what it touches. The sweep has three implicit phases —
intervals ending before the newcomer pass through untouched, intervals it overlaps are swallowed
by widening it, and once an interval starts after the newcomer ends you emit the widened newcomer
and copy the rest. Because the input is already disjoint, this is `O(n)` with no re-sort:

```tool eval (iv_insert [ [1 2] [ [3 5] [ [6 7] [ [8 10] [ [12 16] 0 ] ] ] ] ] [4 8])
```

The lone newcomer `[4 8]` reaches from inside `[3 5]` to inside `[8 10]`, fusing three
intervals into `[3 10]`.

## 2. Scheduling — the sweep line

**Can one person attend every meeting?** Sort by start; if any meeting begins *strictly before*
the previous one ends, there is a conflict. The three sample meetings include `[0 30]`, which
swallows the others, so the answer is no (`1`):

```tool eval (iv_canattend (iv_meetings0 0))
```

**Minimum meeting rooms** is the same data with a deeper question: how many meetings are ever
running *at once*? That peak is exactly the number of rooms you need. The elegant solution
abandons the intervals-as-units view and works with the *event times*: pull every start and
every end into two separate sorted lists, then walk them together. Each start that lands before
the next end claims a room (occupancy up); each end that passes frees one (occupancy down). The
high-water mark is the answer. The sample needs `2` rooms — `[0 30]` overlaps each of the others,
but `[5 10]` and `[15 20]` never coincide, so two rooms suffice:

```tool eval (iv_minrooms (iv_meetings0 0))
```

This **sweep line** — decompose intervals into start/end events, sort the events, and run a
running counter — is one of the most transferable interval techniques; "maximum overlap" of
any kind reduces to it.

## 3. Set operations — intersection and coverage

**Intersecting two sorted interval lists** is a two-pointer sweep. The overlap of the two
intervals currently under the pointers is `[max(starts), min(ends)]`, which is real only when
that low does not exceed that high. Then you advance whichever interval *ends first*, because it
cannot possibly meet anything later — the one that ends later might still overlap the next
interval on the other side. That "advance the earlier-ending one" rule is the crux:

```tool eval (iv_intersect (iv_a0 0) (iv_b0 0))
```

**Total covered length** — the measure of the union, counting overlaps once — is merge plus a
sum: collapse to disjoint pieces, then add each piece's width. The sample covers `(6−1) + (10−8)
+ (18−15) = 10` units of the line:

```tool eval (iv_coverage (iv_sample0 0))
```

That coverage is built straight on top of `iv_merge`, which is the recurring shape of this whole
area: a handful of one-line sweeps, each standing on the same sort-then-scan foundation.

---

Intervals join the rest of the toolkit — paradigms, data-structure patterns, weighted graphs,
number theory, bit manipulation, strings, grids, design, trees, and dynamic programming. Every
arm here is in `lib/intervals.lat` (prefix `iv_`), and `latte intervals <topic>` prints these in
a terminal (topics: merging, scheduling, setops).
