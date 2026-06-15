# Algorithm-design techniques — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel below is a **live
object**: when this text loads, the system evaluates a real Latte expression against
`lib/algo.lat` and embeds what comes back. Edit any command line and middle-click it
to re-run with your own input.

These are the five technique *paradigms* from Steven Skiena's **The Algorithm Design
Manual** — the toolkit behind most coding-interview questions (Toptal included). The
trick interviewers are really testing is **recognising which paradigm a problem
wants**. Each section below names the tell-tale signal, gives the one-line intuition,
and runs a flagship algorithm you can poke at.

Every function lives in `lib/algo.lat`, prefixed `al_`, and is heavily commented. A
word of warning that the comments harp on: in Latte the comparison gates are
*loobeans* where **0 means true**, so `lt a b` is 0 when `a < b`. All the code reads
with that in mind.

## 1. Divide & conquer — split, solve, combine

**Signal:** the problem breaks into independent sub-problems of the same shape.
**Intuition:** solve halves recursively, then spend linear work combining them;
`T(n) = 2T(n/2) + O(n)` collapses to `O(n log n)`.

The cleanest example is **merge sort**. Splitting is trivial; all the cleverness is
in *merging* two already-sorted lists by repeatedly taking the smaller head:

```tool eval (al_msort (al_nums0 0))
```

The same divide-and-conquer halving powers **binary search** — but only on *sorted*
data. Each comparison throws away half the remaining window, so it finds (or refutes)
a key in `O(log n)`. Here it locates `7`, and reports `%nf` for a value that is not
present:

```tool eval (al_bsearch (al_sorted0 0) 7)
```

A subtler relative is **quickselect**: it finds the k-th smallest element *without
fully sorting*, by partitioning around a pivot and recursing into only the side that
contains rank `k`. That "recurse into one side" is what drops the average cost from
`O(n log n)` to `O(n)`. Here `k = 3` picks the median of seven values:

```tool eval (al_qselect (al_nums0 0) 3)
```

**Why the costs come out the way they do — the recursion tree.** Picture merge sort's calls
as a tree: each level splits everything in half, so there are `log n` levels, and every level
together does `O(n)` of merging work — `n` per level × `log n` levels = `O(n log n)`. Binary
search is the degenerate case where you *discard* one half instead of recursing into both, so
the tree is a single path of length `log n` and the total is `O(log n)`. Quickselect sits in
between: it recurses into only *one* side, and when the pivot splits the data evenly the sizes
shrink geometrically — `n + n/2 + n/4 + … = 2n` — so the work sums to `O(n)` on average, even
though the worst case (consistently lopsided pivots) degrades to `O(n²)`. The same `2T(n/2) +
O(n)` recurrence, read through this tree, explains all three.

## 2. Dynamic programming — remember sub-answers

**Signal:** the problem has *overlapping* sub-problems (the naive recursion recomputes
the same thing exponentially often) and *optimal substructure* (a best answer is built
from best sub-answers). **Intuition:** compute each sub-answer once, store it, build up.

**Why both preconditions are needed, semantically.** *Optimal substructure* is what makes a
recurrence *correct*: it lets you assume the sub-answers are themselves optimal and combine
them without second-guessing — coin change relies on "an optimal way to make `n` contains an
optimal way to make `n − coin`", which holds because swapping in a better sub-solution would
beat the supposed optimum. *Overlapping sub-problems* is what makes memoization *pay*: the
naive recursion forms a tree with exponentially many nodes but only *polynomially many
distinct* arguments, so caching collapses the tree into a DAG visited once. Coin change has
`O(amount)` distinct sub-problems and `O(coins)` work each, turning an exponential search into
`O(amount × coins)`. Strip away overlap and you have plain divide & conquer; strip away
optimal substructure and a greedy/DP recurrence is simply *wrong*.

**Coin change** is the gentlest case: the fewest coins to make an amount is one more
than the best way to make "amount minus some coin". We fill a 1-D table from `0` up to
the target so every smaller answer is ready when we need it. Eleven from `{1,5,6,9}`
is just two coins (`5 + 6`):

```tool eval (al_coins (al_coins0 0) 11)
```

Skiena's flagship DP is **edit distance** — the fewest single-character insert, delete,
or substitute edits to turn one string into another (the thing behind spell-checkers
and `diff`). It fills a 2-D table, but each cell only needs the row above and the cell
to its left, so we carry one row at a time. The distance from *kitten* to *sitting* is
the textbook `3`:

```tool eval (al_edit [ %k [ %i [ %t [ %t [ %e [ %n 0 ] ] ] ] ] ] [ %s [ %i [ %t [ %t [ %i [ %n [ %g 0 ] ] ] ] ] ] ])
```

**Longest increasing subsequence** shows the DP recurrence in miniature: the longest
run ending at position `i` is one more than the longest run ending at any earlier,
smaller element. The answer for `[3 1 4 1 5 9 2 6]` is `4` (e.g. `1 4 5 9`):

```tool eval (al_lis [ 3 [ 1 [ 4 [ 1 [ 5 [ 9 [ 2 [ 6 0 ] ] ] ] ] ] ] ])
```

## 3. Greedy — commit to the best local choice

**Signal:** you can prove that a locally-optimal choice is never something you'd want
to undo. **Intuition:** sort by the right key, then sweep once, taking whatever is
safe. Greedy is the fastest paradigm *when it's correct* — and the interview risk is
applying it when it isn't.

The textbook win is **interval scheduling**: given meetings with start/finish times,
fit in the most non-overlapping ones. The provably-correct rule is *earliest finish
first* — the meeting that frees up soonest leaves the most room for the rest. Of these
five intervals only two can coexist:

```tool eval (al_sched (al_ivs0 0))
```

You can see the greedy order it sweeps by sorting the intervals on finish time:

```tool eval (al_isort (al_ivs0 0))
```

**How you actually prove a greedy correct — the exchange argument.** The standard recipe:
take any optimal solution and show you can morph it into the greedy one without making it
worse. For interval scheduling, suppose an optimal schedule's first meeting finishes later
than the earliest-finishing meeting; swap it for the earliest-finisher. The swap can't cause
a conflict (the replacement frees up *sooner*, leaving at least as much room), so the modified
schedule is still valid and still has the same count — therefore still optimal. Repeating the
swap turns *some* optimum into the greedy one, proving greedy is optimal too. This "greedy
stays ahead / exchange" pattern is the tell that a problem is safe for greedy; when no such
swap exists (as in coin change with arbitrary denominations) greedy quietly returns wrong
answers and you must fall back to DP.

## 4. Backtracking — search the whole space, prune early

**Signal:** the problem asks for *all* arrangements, subsets, or a configuration
satisfying constraints. **Intuition:** build a candidate one choice at a time, and the
instant a partial choice can't possibly work, abandon that branch — that pruning is
what separates backtracking from brute force.

The two warm-ups enumerate cleanly. **Subsets** (the power set): each element is either
in or out, so there are `2^n` of them. For `{1,2,3}` that's eight:

```tool eval (al_subsets [ 1 [ 2 [ 3 0 ] ] ])
```

**Permutations** pick each element as the head, then arrange the rest — `n!` results:

```tool eval (al_perms [ 1 [ 2 [ 3 0 ] ] ])
```

The classic that *needs* the pruning is **N-queens**: place `n` queens on an `n×n`
board so none attacks another. We place one per row and reject a column the moment it
shares a file or diagonal with a queen already down — cutting the search from `n^n` to
something tractable. The famous answer for the 8×8 board is `92` solutions:

```tool eval (al_queens 8)
```

Try a smaller board by editing the number — `4` has two solutions, `6` has four:

```tool eval (al_queens 6)
```

## 5. Graph traversal — explore systematically

**Signal:** the data is a network of things and relationships, or the problem mentions
paths, reachability, ordering, or dependencies. **Intuition:** visit nodes in a
disciplined order, keeping a *visited* set so cycles don't trap you.

Our sample is a small DAG: `a → b`, `a → c`, `b → d`, `c → d`, `d → e`.

**Breadth-first search** explores level by level, so the first time it reaches a node
is along a shortest (fewest-edges) path. From `a` to `e` is three hops:

```tool eval (al_bfs (al_graph0 0) %a %e)
```

**Depth-first search** dives as deep as it can before backing up. Here is the order it
visits the nodes from `a`:

```tool eval (al_dfs (al_graph0 0) %a)
```

Finally, **topological sort** orders the nodes so every edge points forward — the
"what must come before what" order you'd use to schedule tasks with dependencies or
compile modules in the right sequence. DFS gives it almost for free: emit each node
only after everything it points to. Notice `a` (a source) lands first and `e` (the
sink) lasts:

```tool eval (al_topo (al_graph0 0) (al_nodes0 0))
```

---

That's the whole toolkit. In an interview the move is always the same two steps: read
the problem for the **signal** above, then reach for the matching paradigm. The code
for every example here is in `lib/algo.lat`, and the `latte algo` command prints all
of these (try `latte algo dp` for just the dynamic-programming set).
