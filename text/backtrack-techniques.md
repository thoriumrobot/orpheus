# Backtracking — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/backtrack.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Backtracking is exhaustive search done intelligently: build a candidate solution one decision
at a time, and the moment a partial choice cannot possibly lead anywhere valid, abandon that
whole branch ("prune") instead of finishing it. The textbook framing is *choose → explore →
un-choose*, where un-choosing restores state before trying the next option. In a **pure
functional** setting the un-choose step vanishes: each option simply begins its own
independent recursive call, nothing is mutated, and a node's complete answer is just the
**concatenation** of its children's answers. That is why the implementations below are so
short — the messy bookkeeping is gone.

The two recurring craft skills are *pruning* (cut dead branches early) and *avoiding
duplicates* (impose a canonical order so the same solution is never generated two ways). Watch
for both in each example.

## 1. Build a string — generate valid parentheses

Generating every well-formed string of `n` parenthesis pairs is the cleanest pruning demo.
Track two counters: how many `(` you may still open, and how many `)` you currently owe. Then
only ever open while opens remain, and only ever close while you owe a close — so an
*invalid* prefix like `())` is never even constructed. The number of results is the
**Catalan number** `Cₙ`, which is the whole reason this problem is famous:

```tool eval (len (bk_parens 3))
```

Three pairs give `C₃ = 5`; four give `14`:

```tool eval (len (bk_parens 4))
```

(The strings themselves are real cords; multi-byte cords print as integers here, so we count
them.) The pruning is doing real work — without it you would generate all `2^(2n)` bracket
strings and filter, exponentially more than the Catalan many that survive.

## 2. Pick — combinations and combination sum

**Combination sum** finds every multiset of candidates (each reusable) that hits a target. The
duplicate-avoidance trick is subtle and worth internalising: the "take this candidate" branch
keeps the *same* candidate available, while the "skip" branch drops it permanently — so within
any single solution the chosen values never decrease, and `{2,2,3}` is generated once rather
than in all its orderings. For `{2,3,6,7}` targeting `7`:

```tool eval (bk_combsum (bk_cands0 0) 7)
```

The plain **k-combinations** of `{1..n}` use the same increasing-order idea, branching on each
number as include-or-exclude. All the 2-subsets of `{1,2,3,4}`:

```tool eval (bk_combine 4 2)
```

Both lean on the identity that the number of k-subsets is `C(n,k)` — exclude-or-include at each
element is exactly Pascal's recurrence `C(n,k) = C(n−1,k−1) + C(n−1,k)`, which is *literally*
the two branches.

## 3. Split a string — palindrome partitioning

Here a "decision" is where to make the first cut. For every prefix that happens to be a
palindrome, treat it as the first piece and recursively partition the remaining suffix; gluing
the prefix onto each sub-result gives every valid partition. The empty string seeds the
recursion with a single empty partition. `aab` splits two ways (`a|a|b` and `aa|b`):

```tool eval (len (bk_partition %aab))
```

…while `aaa` splits four ways, because more prefixes are palindromic:

```tool eval (len (bk_partition %aaa))
```

This "try every prefix, recurse on the rest" shape recurs across string-DP and parsing problems;
backtracking is just the version that enumerates *all* splits rather than scoring one.

## 4. Grid path — word search

The richest backtracking flavour adds a *visited set*. To find a word in a letter grid as a
path of orthogonally adjacent cells with no cell reused, DFS from each starting square: if the
current cell matches the current letter, mark it used and recurse into the four neighbours for
the next letter; on the way out the mark is gone automatically (a fresh `used` set per branch).
On the board `ABCE / SFCS / ADEE`, the word `ABCCED` snakes through and is found:

```tool eval (bk_wordsearch (bk_grid0 0) %ABCCED)
```

But `ABCB` is *not* present — the only way to reach a second `B` would reuse the first cell, and
the used-set forbids it (`1` = absent):

```tool eval (bk_wordsearch (bk_grid0 0) %ABCB)
```

That used-set discipline — mark on entry, and let each branch carry its own copy so siblings do
not see each other's marks — is the heart of every grid, maze, and constraint backtracking
problem, N-queens included.

---

Backtracking joins the rest of the toolkit — paradigms, data-structure patterns, weighted
graphs, number theory, bit manipulation, strings, grids, design, trees, dynamic programming,
intervals, binary search, and graph problems. Every arm here is in `lib/backtrack.lat` (prefix
`bk_`), and `latte backtrack <topic>` prints these in a terminal (topics: parens, combos,
partition, grid).
