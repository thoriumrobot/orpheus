# Data structures & coding patterns — an interactive tour

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/dsa.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

This is the companion to the *algorithm-techniques* tour. That one covered the five
*paradigms* (the high-level strategy); this one covers the **patterns and data
structures** — the shapes interview problems actually arrive in, and the structures
that make them fast. The single most useful interview skill is matching a problem to
its pattern, so each section below leads with the **recognition cue**.

## How to pick a pattern

Read the problem and look for the tell-tale phrase:

- "find a pair / sorted array / in place" → **two pointers**
- "subarray / substring / longest / of size k" → **sliding window**
- "linked list / cycle / find the middle" → **fast & slow pointers**
- "matching / nesting / evaluate / next greater" → **stack**
- "many range-sum queries over a fixed array" → **prefix sums**
- "top-K / k smallest / merge k sorted / median" → **heap**
- "keep ordered / insert + lookup + in-order" → **binary search tree**
- "prefix / autocomplete / dictionary" → **trie**
- "connected? / groups / islands / merge sets" → **union-find**

The rest of this tour walks each one with a runnable example. A reminder that the code
comments harp on: in Latte the comparison gates are *loobeans* where **0 means true**,
and `sub` errors on underflow — every function is written with that in mind.

## 1. Two pointers — a sorted array, scanning inward

**Cue:** the data is sorted and you need a pair, or you're shrinking/growing from the
ends. **Why it works:** one pointer at each end means each comparison rules out a whole
direction, turning an `O(n²)` double loop into one `O(n)` sweep.

Pair sum: do two values in the sorted list `[2 7 11 15]` add to 9? The pointers start
wide; `2 + 15` is too big so the right pointer steps in, `2 + 11` still too big, then
`2 + 7 = 9` — indices `[0 1]`:

```tool eval (ds_pairsum (ds_arr0 0) 9)
```

The same two-pointer idea checks a **palindrome** by comparing the ends inward
(`1` means yes):

```tool eval (ds_ispalin [ %r [ %a [ %c [ %e [ %c [ %a [ %r 0 ] ] ] ] ] ] ])
```

## 2. Sliding window — contiguous runs

**Cue:** "subarray" or "substring", and "longest/shortest/max" or "of size k". **Why it
works:** instead of recomputing each window from scratch, you adjust the previous answer
by what entered and what left — `O(n)` overall.

Fixed window: the largest sum of any two adjacent elements of `[2 1 5 1 3 2]` is `6`
(the `5 1` window slid into; `1 5` and `5 1` both give 6):

```tool eval (ds_maxwin (ds_win0 0) 2)
```

Variable window: the longest stretch of `"abcabcbb"` with no repeated character is `3`.
The window grows until a repeat appears, then shrinks from the left until it's unique
again:

```tool eval (ds_longestuniq (ds_str0 0))
```

**Why it is O(n), not O(n²) — the amortised argument.** It looks like a nested loop: an outer
right pointer and an inner left pointer that shrinks the window. But the left pointer only
ever moves *forward*, and it can never pass the right pointer, so across the whole run each of
the two pointers advances at most `n` times total. The work is therefore `O(n)` even though
any single right-step might trigger several left-steps — the same amortised reasoning behind
the two-stack queue in the *design* tour. The invariant that makes the answer correct is that
the window `[left, right]` is *always* kept valid (here: free of duplicates) after each step,
so whenever we measure its length we are measuring a genuine candidate.

## 3. Fast & slow pointers — cycles in a linked list

**Cue:** a linked list, and "does it cycle?" or "find the middle". **Why it works:** if
one pointer moves twice as fast as the other, then inside a loop the fast one gains one
step per tick and must eventually land on the slow one — Floyd's tortoise and hare, in
`O(1)` extra space. (A Latte value can't point back to itself, so the list is given as a
successor map.)

**Why the gap closes exactly (the semantic detail).** Once both pointers are inside a cycle
of length `L`, look at the *gap* between them measured forward around the loop. Every tick the
fast pointer advances 2 and the slow advances 1, so the gap shrinks by exactly 1 each tick.
A quantity that starts somewhere in `0..L-1` and strictly decreases by 1 (mod `L`) must hit 0
within `L` steps — that is the moment they collide. The argument also shows why a *2:1* ratio
specifically is guaranteed to meet: any step difference that shares no common factor with `L`
would still close, but the difference of 1 produced by 2-vs-1 closes for *every* `L`, so it
needs no assumptions about the loop's size. Detecting a cycle is therefore O(n) time and O(1)
space, beating the O(n) space of a "seen" set.

A list `a→b→c→d→b` loops back, so a cycle exists (`1`):

```tool eval (ds_hascycle (ds_cyclic 0) %a)
```

A list `a→b→c→nil` runs off the end, so no cycle (`0`):

```tool eval (ds_hascycle (ds_acyclic 0) %a)
```

## 4. Stack — matching, evaluating, "next greater"

**Cue:** nesting that must match, an expression to evaluate, or "next greater/smaller
element". **Why it works:** a stack remembers the most recent unfinished thing, which is
exactly what closers, operators, and monotonic scans need.

Balanced brackets: every closer must pop its exact opener, and the stack must end empty.
`([]{})` is balanced (`1`):

```tool eval (ds_balanced (ds_bal0 0))
```

…while `([)]` is not (`0`) — the `)` finds a `[` on top:

```tool eval (ds_balanced (ds_bal1 0))
```

Reverse-Polish evaluation: push numbers, and an operator pops its two operands. `3 4 + 5 *`
is `(3+4)*5 = 35`:

```tool eval (ds_rpn (ds_rpn0 0))
```

A **monotonic** stack finds, for each element, the next element to its right that is
larger. For `[2 1 3]` that's `[3 3 %none]` (3 is next-greater for both 2 and 1; 3 has none):

```tool eval (ds_nextgreater [ 2 [ 1 [ 3 0 ] ] ])
```

## 5. Prefix sums — O(1) range queries

**Cue:** lots of "sum of the elements from i to j" questions over an array that doesn't
change. **Why it works:** if `P[i]` is the running total of the first `i` elements, any
range sum is just `P[j] - P[i]`.

The prefix table of `[2 4 6]` is `[0 2 6 12]`:

```tool eval (ds_prefix [ 2 [ 4 [ 6 0 ] ] ])
```

…and the sum over the half-open range `[1,3)` (the `4` and the `6`) is `10`, with no
re-adding:

```tool eval (ds_rangesum (ds_prefix [ 2 [ 4 [ 6 0 ] ] ]) 1 3)
```

## 6. Heap — top-K and always-the-minimum

**Cue:** "the k smallest/largest", "merge k sorted lists", "running median". **Why it
works:** a heap keeps the extreme element at the root, so you can pull the min (or max)
in `O(log n)`. This one is a *leftist heap* — fully functional, no mutation.

Pulling the minimum repeatedly **is** a sort:

```tool eval (ds_heapsort (ds_unsort0 0))
```

…and stopping after three pulls gives the three smallest:

```tool eval (ds_topk (ds_unsort0 0) 3)
```

## 7. Binary search tree — ordered lookups

**Cue:** you need to keep a changing set ordered, with fast insert and lookup, and
sometimes "in sorted order". **Why it works:** the left-less-than-right invariant means
every operation follows one root-to-leaf path.

An in-order traversal visits the keys in sorted order, so a BST built from `5,2,8,1,9`
reads back as `[1 2 5 8 9]`:

```tool eval (ds_bstinorder (ds_bstins (ds_bstins (ds_bstins (ds_bstins (ds_bstins 0 5) 2) 8) 1) 9))
```

Lookup follows the same comparisons — `8` is present (`0` = yes):

```tool eval (ds_bsthas (ds_bstins (ds_bstins 0 5) 8) 8)
```

## 8. Trie — prefixes and dictionaries

**Cue:** "prefix", "autocomplete", or matching against a dictionary of words. **Why it
works:** shared prefixes share a path, so a query costs only its own length — never the
size of the dictionary.

Store `cat` and `car`, then ask whether `cat` is a stored word (`0` = yes):

```tool eval (ds_thas (ds_tins (ds_tins (ds_tnew 0) [ %c [ %a [ %t 0 ] ] ]) [ %c [ %a [ %r 0 ] ] ]) [ %c [ %a [ %t 0 ] ] ])
```

`ca` is not itself a word, but it **is** a prefix of stored words (`0` = yes, it's a
prefix):

```tool eval (ds_tprefix (ds_tins (ds_tnew 0) [ %c [ %a [ %t 0 ] ] ]) [ %c [ %a 0 ] ])
```

## 9. Union-Find — connectivity and groups

**Cue:** "are these connected?", "how many groups/islands?", "merge these sets". **Why
it works:** each element points toward a group representative; merging groups is just
re-pointing one root at another, and a connectivity test compares roots.

**The remarkable complexity, and where it comes from.** Two refinements make the operations
*almost* free. *Union by rank* always hangs the smaller tree under the larger, so trees never
get taller than `O(log n)`. *Path compression* re-points every node you pass during a `find`
directly at the root, so the very act of querying flattens the structure for next time —
the work you do is never wasted. Together they yield an amortised cost of `α(n)`, the inverse
Ackermann function, which is below 5 for any `n` that could fit in the observable universe —
effectively constant. It is one of the rare data structures whose tight bound is genuinely
surprising, and a favourite interview "why is this fast?" follow-up. (The version in `dsa.lat`
keeps the plain forest so the core idea stays visible; the refinements are a small add-on.)

Start with `{a,b,c,d}`, union `a–b` and `c–d`, and there are `2` groups left:

```tool eval (ds_ufcount (ds_ufunion (ds_ufunion (ds_ufnew [ %a [ %b [ %c [ %d 0 ] ] ] ]) %a %b) %c %d))
```

After uniting `a` and `b`, they test as connected (`0` = same group):

```tool eval (ds_ufconnected (ds_ufunion (ds_ufnew [ %a [ %b [ %c 0 ] ] ]) %a %b) %a %b)
```

---

That's the pattern toolkit. Paired with the five paradigms in the *algorithm-techniques*
tour, it covers the techniques a coding interview leans on. The whole move is two steps:
spot the **cue**, reach for the matching structure. Every example here is in
`lib/dsa.lat`, and `latte dsa <topic>` prints these in a terminal (topics: pointers,
stack, prefix, heap, bst, trie, unionfind).
