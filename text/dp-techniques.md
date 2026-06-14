# Dynamic programming — an interactive tour

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/dp.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Dynamic programming is the most feared interview category and the most mechanical once you
see its shape. Every DP is the same two-part move:

1. **Find a recurrence** — express the answer to a problem in terms of answers to *smaller*
   versions of the same problem. This requires *optimal substructure*: an optimal whole is
   built from optimal parts.
2. **Stop recomputing** — because those sub-problems *overlap* (the naive recursion asks the
   same question exponentially often), compute each distinct sub-answer once and store it.

That is the whole subject. What changes from problem to problem is only the recurrence and the
shape of the table you fill. The hard part of an interview DP is almost never the code — it is
*spotting the recurrence*. The tours below are grouped by what the recurrence computes.

## 1. One-dimensional DP — a single rolling index

**Climbing stairs** is the gentlest DP. To reach step `n` your last move was either a single
step (from `n-1`) or a double (from `n-2`), and those are the only options, so
`ways(n) = ways(n-1) + ways(n-2)` — the Fibonacci recurrence in disguise. Because each answer
needs only the previous two, we carry a rolling pair instead of a whole table, giving `O(n)`
time and `O(1)` space:

```tool eval (dp_stairs 5)
```

**House robber** introduces a *choice* at each step: rob this house (and forfeit its
neighbour) or skip it. The recurrence keeps two running bests — `take` = best ending in a
robbed house, `skip` = best ending in a skipped one — and the decision at each house is purely
local: `take` becomes `skip + value`, `skip` becomes `max(take, skip)`. The maximum
non-adjacent sum of `[2 7 9 3 1]` is `2 + 9 + 1 = 12`:

```tool eval (dp_rob (dp_houses0 0))
```

The lesson: a 1-D DP often needs only a constant slice of the table, because the recurrence
reaches back only a fixed distance.

## 2. Feasibility — can we hit a target exactly?

**Subset-sum** asks whether *some* subset reaches an exact target. The DP tracks the **set of
reachable sums**: start with `{0}` (the empty subset), and each new number `x` makes every
already-reachable `s` also reach `s + x`. The target is achievable iff it survives in that
set. From `{3,34,4,12,5,2}`, the sum `9` is reachable (`4 + 5`), so the answer is yes (`0`):

```tool eval (dp_subsetsum (dp_set0 0) 9)
```

**Equal partition** — split a list into two equal-sum halves — looks different but *reduces*
to subset-sum: a split exists exactly when the total is even and some subset reaches half of
it. Recognising that one problem is a thin wrapper over another is itself a core DP skill:

```tool eval (dp_canpartition (dp_part0 0))
```

## 3. Counting — how many ways?

**Coin-change ways** counts the distinct combinations summing to an amount. There is a famous
trap here. If you loop *amounts* on the outside and coins inside, you count *permutations*
(`1+2` and `2+1` separately). Looping **coins on the outside** — finishing one coin before
touching the next — counts *combinations*, because a given coin's contributions are all added
before the next coin is ever considered. Within a coin, sweeping amounts low-to-high and
reading the just-updated `dp[a-c]` lets that coin be reused freely. Making `5` from `{1,2,5}`
has four combinations:

```tool eval (dp_coinways (dp_coins0 0) 5)
```

The base case is the quietly important one: there is exactly **one** way to make `0` — use no
coins at all. That `dp[0] = 1` seed is what makes every other count come out right:

```tool eval (dp_coinways (dp_coins0 0) 0)
```

## 4. Optimisation — best value under a constraint

**0/1 knapsack** is the flagship. Choose items (each with a weight and a value) to maximise
total value without exceeding a capacity, using each item at most once. `dp[w]` is the best
value achievable with capacity `w`; folding in an item, each capacity becomes
`max(leave the item, take it = dp[w - weight] + value)`. The decisive detail is that **the
take-branch reads the table from *before* this item was considered** — which is exactly what
forbids using an item twice. (An *unbounded* knapsack, where items may repeat, is the same
code reading the table *being built* — a one-character conceptual change.) Capacity 5 over
items `(2,3)(3,4)(4,5)(5,6)` is best filled by the first two, value `7`:

```tool eval (dp_knapsack (dp_items0 0) 5)
```

**Longest common subsequence** is the canonical *two-dimensional* DP — the engine behind
`diff` and DNA alignment. Comparing prefixes of two strings, `lcs(i,j)` is `lcs(i-1,j-1)+1`
when the characters match (extend the common run) and `max(lcs(i-1,j), lcs(i,j-1))` when they
do not (drop a character from one side). Filling that grid row by row — keeping only the
previous row — costs `O(|a|·|b|)`. The LCS of `AGGTAB` and `GXTXAYB` is `GTAB`, length `4`:

```tool eval (dp_lcs %AGGTAB %GXTXAYB)
```

The shared structure across all four families is worth stating plainly: define the sub-problem,
write the recurrence, identify the base case, and choose an order that fills each cell only
after the cells it depends on. Master that checklist and DP stops being a category you fear.

---

Dynamic programming joins the rest of the toolkit — paradigms, data-structure patterns,
weighted graphs, number theory, bit manipulation, strings, grids, design, and trees. Every arm
here is in `lib/dp.lat` (prefix `dp_`), and `latte dp <topic>` prints these in a terminal
(topics: oned, feasibility, counting, optimisation).
