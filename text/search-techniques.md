# Binary search & its variants — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/search.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Plain binary search — "find this key in this sorted array" — is the one algorithm everyone
knows. The *interview* skill is recognising the disguises: the same halving idea applies
anywhere there is a **monotone boundary**, even when there is no obvious sorted array at all.
The single invariant behind every variant below is: *keep a range that is guaranteed to
contain the answer, and shrink it by half each step by asking one yes/no question at the
middle.* Getting that boundary condition exactly right — `<` vs `<=`, `mid` vs `mid+1` — is
where most binary-search bugs live, which is why these are worth practising as their own
family.

## 1. Boundaries — where a value's run begins and ends

With duplicates, "find the key" splits into two sharper questions. **Lower bound** is the
first index whose value is `>=` target — equivalently, the count of elements strictly smaller,
and the index of the *first* occurrence if the target is present. The trick is the comparison:
when `xs[mid] < target` the boundary is strictly to the right, otherwise `mid` itself stays a
candidate, so we never discard a possible answer. In `[1 2 2 2 3 4]` the first `2` sits at
index 1:

```tool eval (se_lowerbound (se_sorted0 0) 2)
```

**Upper bound** is the same search with `<=` at the midpoint, giving the index *one past* the
last occurrence. The two together do something neither does alone — they **count** a value in
`O(log n)` without ever scanning its run: `count = upper − lower`. There are three `2`s:

```tool eval (se_count (se_sorted0 0) 2)
```

That lower/upper-bound pair is the workhorse; membership, counting, and "insert position" are
all one of them.

## 2. Rotated arrays — half is always sorted

A sorted array that has been cut and swapped — `[4 5 6 7 0 1 2]` — is no longer globally
sorted, yet binary search still applies. The key observation: **at any midpoint, at least one
of the two halves is still in sorted order** (the rotation point can only be on one side). So
you identify the sorted half by comparing `xs[lo]` to `xs[mid]`, check whether the target lies
within that half's known range, and recurse accordingly. `0` lives at index 4:

```tool eval (se_rotated (se_rot0 0) 0)
```

A cousin question — **find the minimum** — is the rotation point itself. Compare `xs[mid]` to
`xs[hi]`: if `xs[mid] > xs[hi]` the array must "wrap" somewhere to the right of `mid`, so the
minimum is there; otherwise it is at `mid` or to its left. Comparing against `xs[hi]` rather
than `xs[lo]` is what makes the not-actually-rotated case fall out correctly:

```tool eval (se_rotmin (se_rot0 0))
```

## 3. Slope — binary search with no sorted array at all

**Finding a peak** (an element greater than both neighbours) looks impossible to binary-search,
because the array is *not* sorted. But you do not need global order — you need a guaranteed
direction of improvement. If `xs[mid] < xs[mid+1]` you are walking uphill, and since the array
edges count as `-∞`, going uphill *must* eventually reach a peak; so a peak exists to the right.
Otherwise `mid` is on a downslope and is itself a valid peak or has one to its left. Either way
half the array is eliminated. `[1 2 3 1]` peaks at index 2:

```tool eval (se_peak (se_peak0 0))
```

The lesson generalises: binary search needs a *monotone decision*, not a sorted array.

## 4. Binary search on the answer — the array is imaginary

This is the most powerful and least obvious variant. When the answer is a number in a known
range, and the predicate "**is value `k` good enough?**" is **monotone** (once true it stays
true as `k` grows, or vice-versa), you can binary-search the *answer itself* — there is no
array, just the number line.

**Integer square root** is the gentle case: find the largest `k` with `k·k ≤ n`. The predicate
`k·k ≤ n` is true for small `k` and flips to false past `√n`, so we hunt that boundary. For
`17`, the answer is `4` (since `16 ≤ 17 < 25`):

```tool eval (se_isqrt 17)
```

The classic interview version is **Koko eating bananas**: given piles and a deadline of `h`
hours (one pile per hour-block, `ceil(pile/speed)` hours each), find the minimum eating speed
that finishes in time. "speed `k` is fast enough" is monotone — faster never hurts — so we
binary-search the smallest good speed between `1` and the biggest pile. For piles `[3 6 7 11]`
within `8` hours, the slowest workable speed is `4`:

```tool eval (se_minspeed (se_piles0 0) 8)
```

The recipe to take into an interview: when a problem says "minimum/maximum X such that Y is
achievable", and Y gets easier as X grows, you are almost certainly meant to binary-search X
and write a feasibility check — turning an `O(answer)` scan into `O(log answer)` checks.

---

Search joins the rest of the toolkit — paradigms, data-structure patterns, weighted graphs,
number theory, bit manipulation, strings, grids, design, trees, dynamic programming, and
intervals. Every arm here is in `lib/search.lat` (prefix `se_`), and `latte search <topic>`
prints these in a terminal (topics: boundaries, rotated, slope, answer).
