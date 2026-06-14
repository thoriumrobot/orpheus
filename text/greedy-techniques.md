# Greedy algorithms — an interactive tour

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/greedy.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

A greedy algorithm commits to whatever looks best *right now* and never backtracks. When that
works it is gloriously simple — usually one linear pass where dynamic programming would need a
whole table. The catch, and the entire intellectual content of the subject, is *proving the
greedy choice is safe*. The standard proof is an **exchange argument**: take any optimal
solution, show you can rewrite it to agree with the greedy pick without making it worse, and
conclude that some optimal solution contains the greedy choice. If you cannot make that
argument, greed is probably wrong and you need DP. Each example below pairs the one-line sweep
with the reason it is allowed.

## 1. Reachability — the jump game

Given an array where `xs[i]` is the maximum forward jump from `i`, **can you reach the last
index?** Sweep left to right tracking the single furthest index reachable so far. If the cursor
ever stands beyond that reach, you are stranded; otherwise you coast to the end. The exchange
argument is almost trivial: remembering the *furthest* reach never discards a reachable index,
so a greedy that only tracks that maximum loses nothing. `[2 3 1 1 4]` is reachable:

```tool eval (gd_canjump (gd_jump0 0))
```

…but `[3 2 1 0 4]` stalls at the `0` in the middle (`1` = stuck):

```tool eval (gd_canjump (gd_jump1 0))
```

The **minimum-jumps** variant is the same sweep read as a breadth-first layering: the indices
reachable in `k` jumps form a contiguous range, and each time the cursor crosses the current
range's right edge you have spent one more jump. `[2 3 1 1 4]` needs `2` (`0→1→4`):

```tool eval (gd_minjumps (gd_jump0 0))
```

## 2. Circuit — the gas station

A circular route: `gas[i]` fuel at station `i`, `cost[i]` to drive to the next. **Where can you
start to complete the loop?** Two greedy facts collapse this to one pass. First, it is feasible
*at all* exactly when total gas ≥ total cost. Second — the real insight — if the tank runs dry
somewhere between a start `s` and station `i`, then *no* station in `[s, i]` can work either
(each would arrive at the failure point with even less fuel), so the only candidate left is
`i+1`. With the valid sample the answer is station `3`:

```tool eval (gd_gas (gd_gasG0 0) (gd_gasC0 0))
```

When total fuel falls short, the function reports `n` (the array length) to signal
impossibility — there is simply no surplus to draw on no matter where you begin.

## 3. Scanning — partition labels, and two-pass candy

**Partition labels** cuts a string into as many parts as possible so that no letter appears in
two parts. Precompute each letter's last position; then scan, pushing the current part's right
edge out to the last occurrence of every letter seen, and close the part the instant the cursor
reaches that edge. It is forced — the part *cannot* end earlier without splitting some letter —
which is exactly why the greedy boundary is optimal. The classic string splits as `9, 7, 8`:

```tool eval (gd_partition %ababcbacadefegdehijhklij)
```

**Candy** is the canonical *two-pass* greedy. Each child needs at least one candy and must
out-candy any strictly lower-rated neighbour; minimise the total. The two neighbour
constraints — beat your left neighbour, beat your right neighbour — are independent, so satisfy
each with its own one-directional sweep and give every child the **maximum** of the two demands.
That max is the smallest number meeting both rules at once. Ratings `[1 0 2]` need `5` candies
(`2,1,2`):

```tool eval (gd_candy (gd_rat0 0))
```

Neither single pass alone is correct; it is the *combination* (and the proof that the
constraints don't interact) that makes the greedy valid.

## 4. Counting — the task scheduler

Given how often each task must run and a cooldown of `n` slots between repeats of the same task,
find the minimum total time including forced idles. Here the greedy insight is structural rather
than a sweep: **the most frequent task dictates the schedule.** Lay it down `(max−1)` times with
`n` idle-or-other slots after each, forming `(max−1)` frames of width `(n+1)`, then a final frame
holding every task tied for that maximum. But if there are *enough* other tasks to fill all the
idle gaps, no idling happens at all and the time is just the task count — so the answer is the
larger of the two. Two tasks run three times each with cooldown `2` take `8` (`AB_AB_AB`):

```tool eval (gd_taskschedule (gd_freqs0 0) 2)
```

Drop the cooldown to `0` and the gaps vanish — the answer falls to the raw task count, `6`:

```tool eval (gd_taskschedule (gd_freqs0 0) 0)
```

The recurring lesson across all four: a greedy solution is only as good as its justification.
When you can show the local choice never forecloses an optimum — by exchange, by forcing, or by
a counting bound — you trade a DP table for a single clean pass.

---

Greedy joins the rest of the toolkit — paradigms, data-structure patterns, weighted graphs,
number theory, bit manipulation, strings, grids, design, trees, dynamic programming, intervals,
binary search, graph problems, and backtracking. Every arm here is in `lib/greedy.lat` (prefix
`gd_`), and `latte greedy <topic>` prints these in a terminal (topics: jump, gas, scan,
schedule).
