# Matrix & grid algorithms — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/grid.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Grid problems are a top-frequency interview category, and they reward one insight: a grid
*is* a graph in disguise. Each cell is a node, adjacent to its four neighbours, so the same
traversal ideas from the graph material reappear — alongside a flavour of dynamic
programming and some array surgery.

In this module a grid is a **list of rows**, each row a list of cell values, addressed by
`(r, c)` with both indices 0-based. A visited set is an assoc map keyed by the flattened
index `r*cols + c`. Results follow the loobean convention where **0 means true**.

## 1. Connectivity — number of islands via flood fill

**Cue:** "count the regions", "connected components", "number of islands", "flood fill /
paint bucket". **Idea:** scan every cell; the first time you touch an unvisited piece of
land, you have found a new island — so *flood* its entire connected component (marking each
cell visited so you never recount it) and add one to the tally.

The sample grid holds land (`1`) and water (`0`) in a shape with three separate islands —
a top-left L, a lone cell, and a bottom-right hook:

```tool eval (gr_islands (gr_grid0 0))
```

The flood itself uses an explicit stack rather than recursion (so a huge landmass cannot
overflow), and only enqueues neighbours that are in bounds — the trickiest part in a
language where subtracting below zero is an error. The connectivity is **4-directional**, so
cells that touch only at a corner are *different* islands:

```tool eval (gr_islands [ [1 [0 0]] [ [0 [1 0]] 0 ] ])
```

That diagonal pair returns 2, not 1 — a classic place to get the adjacency rule wrong.

**Why a single scan with flooding is correct and linear.** Two invariants do the work. First,
*completeness*: when flood fill starts at a land cell it does not stop until the worklist is
empty, and it enqueues every in-bounds land neighbour of every cell it marks — so it provably
reaches every cell reachable by 4-steps, i.e. the entire connected component, nothing more,
nothing less (whether you drain the worklist as a stack/DFS or a queue/BFS, the *set* reached
is identical). Second, *no double counting*: a cell is marked visited the instant it is
processed, so the outer scan can only ever start a new flood from a cell no previous flood
touched. Each cell is therefore visited O(1) times across the whole run, giving O(rows·cols)
total — optimal, since you must at least look at every cell once.

## 2. Grid dynamic programming — unique paths

**Cue:** "how many ways to reach the corner", "robot moving right/down", "count the paths".
**Idea:** the number of distinct routes to a cell, moving only right or down, is the routes
to the cell above plus the routes to the cell on the left: `paths(i,j) = paths(i-1,j) +
paths(i,j-1)`, with the first row and column all 1. Keeping just the previous row gives an
`O(rows·cols)` time, `O(cols)` space solution. A 3×3 grid has six paths:

```tool eval (gr_pathcount 3 3)
```

The count grows combinatorially — it is exactly `C(rows+cols-2, rows-1)` — so a 3×7 grid
already has 28:

```tool eval (gr_pathcount 3 7)
```

## 3. Transforms — transpose, rotate, spiral

**Cue:** "rotate the image in place", "print the matrix in spiral order", "transpose".
**Transposing** swaps rows and columns — the new row `c` is the old column `c`:

```tool eval (gr_transpose (gr_mat0 0))
```

**Rotating 90° clockwise** is then just a transpose followed by reversing each row, so the
old top row ends up as the right-hand column:

```tool eval (gr_rotate (gr_mat0 0))
```

**Spiral order** is the elegant one. Instead of juggling four shrinking boundaries, emit the
top row, then rotate the rest counter-clockwise and repeat — what was the right column is now
the new top row. The recursion peels the matrix inward with no index arithmetic at all:

```tool eval (gr_spiral (gr_mat0 0))
```

That walks `1 2 3` across the top, down the right side to `9`, back along the bottom, up the
left, and finishes at the centre `5`.

---

Matrix/grid joins the rest of the interview toolkit: paradigms, data-structure patterns,
weighted graphs, number theory, bit manipulation, string algorithms, and now grids. Every
arm here is in `lib/grid.lat` (prefix `gr_`), and `latte grid <topic>` prints these in a
terminal (topics: connectivity, dp, transforms).
