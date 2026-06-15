# Tutorials — the interactive index

Every tutorial below is a **live text**: open it and its framed commands run against the real
libraries as the text loads, so you read the explanation and see the actual results together.
Middle-click any `System.OpenText …` line, or click its button, to open one. They are grouped by
theme; each is self-contained, so start anywhere.

**New to all this?** Begin with the primer — it teaches how to read the code, the `0 = yes`
convention, and how to run the live examples, in about ten minutes:
[▸ Start here](run: System.OpenText start-here)

## Foundations & paradigms

[Algorithm paradigms ▸](run: System.OpenText algorithm-techniques) — Skiena's five: divide & conquer, dynamic programming, greedy, backtracking, graph traversal.
[Data structures & patterns ▸](run: System.OpenText dsa-techniques) — two pointers, sliding window, fast & slow, stacks, heaps, BST, trie, union-find.

## Core techniques

[Dynamic programming ▸](run: System.OpenText dp-techniques) — the recurrence recipe: stairs, house robber, subset-sum, coin-change ways, 0/1 knapsack, LCS.
[Greedy algorithms ▸](run: System.OpenText greedy-techniques) — the exchange argument: jump game, gas station, partition labels, candy, task scheduler.
[Backtracking ▸](run: System.OpenText backtrack-techniques) — choose/explore as concatenated branches: parentheses, combination sum, palindrome partition, word search.
[Binary search & variants ▸](run: System.OpenText search-techniques) — lower/upper bound, rotated arrays, peak finding, and searching the answer space.

## Graphs

[Weighted graphs ▸](run: System.OpenText weighted-graphs) — shortest paths and minimum spanning trees: Dijkstra, Kruskal, Prim.
[Graph decision problems ▸](run: System.OpenText graph-techniques) — topological sort, cycle / course-schedule detection, components, bipartite 2-colouring.

## Strings, intervals & grids

[String algorithms ▸](run: System.OpenText string-techniques) — palindrome, anagram, Rabin-Karp rolling hash, longest common prefix, run-length encoding.
[Interval techniques ▸](run: System.OpenText interval-techniques) — merge, insert, meeting rooms (sweep line), intersection, coverage.
[Grid & matrix ▸](run: System.OpenText grid-techniques) — number of islands (flood fill), grid path DP, transpose, rotate, spiral.

## Math & bits

[Number theory ▸](run: System.OpenText number-theory) — GCD/LCM, the sieve, modular exponentiation, binomials, Fibonacci.
[Bit manipulation ▸](run: System.OpenText bit-manipulation) — XOR tricks, add without `+`, power-of-two, bitmask subsets, popcount DP.

## Structures & design

[Binary trees ▸](run: System.OpenText tree-techniques) — traversals, height, invert, max path sum, lowest common ancestor, validate-BST.
[Data-structure design ▸](run: System.OpenText design-techniques) — min-stack, a queue from two stacks, an LRU cache.

## Systems

[The composed database ▸](run: System.OpenText database-tutorial) — the LSM + WAL + Bloom + secondary-index + MVCC database, built and queried step by step.

---

The catalog behind these tutorials — every module's arms, the representation conventions, and the
recipe for adding your own — is in the manual `interview-techniques` (open it with
[the manual ▸](run: System.Edit interview-techniques)). Each tutorial also has a matching CLI
demo: `latte <module> <topic>`.
