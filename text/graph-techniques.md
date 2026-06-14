# Graph problems — an interactive tour

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/graphs.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Graphs are the deepest single category in interviews, but the questions cluster into a few
recurring shapes. The `algo` module covers raw traversal; this tour covers the structural
*questions* you ask about a graph — its ordering, its cycles, its pieces, its colourability.
Throughout, a graph is an adjacency map from each node to its out-neighbours; undirected graphs
simply record every edge in both directions.

## 1. Ordering and cycles — directed graphs

A **topological sort** lists the nodes of a directed graph so that every edge points forward —
the order you could take courses so that prerequisites always come first. **Kahn's algorithm**
builds it from the notion of *in-degree* (how many edges point *at* a node). A node with
in-degree zero depends on nothing, so it can go next; removing it may drop other nodes to
in-degree zero, and you repeat. The sample is the classic prerequisite DAG, and one valid order
is:

```tool eval (gp_toposort (gp_dag0 0))
```

The beautiful part is what happens when the graph has a **cycle**: the nodes in the cycle
forever point at each other, so none of them ever reaches in-degree zero, and Kahn gets stuck
having emitted *fewer* than all the nodes. That gives a cycle test for free — "**can all
courses be finished?**" is exactly "did topological sort emit every node?". The acyclic sample
passes; a graph with the 3-cycle `0→1→2→0` fails:

```tool eval (gp_isdag (gp_cyc0 0))
```

(`0` = acyclic / all finishable, `1` = a cycle blocks completion.) The same in-degree counting
underlies both — ordering and cycle detection are one algorithm read two ways.

## 2. Components and reachability — undirected graphs

A **connected component** is a maximal set of mutually reachable nodes. To count them, repeatedly
**flood-fill** from any node not yet seen — marking everything reachable — and tally how many
floods it takes to cover the graph. Each flood paints exactly one component, so the flood count
*is* the component count. The sample has a triangle `{0,1,2}`, an edge `{3,4}`, and a lone node
`{5}` — three pieces:

```tool eval (gp_components (gp_undir0 0))
```

The same flood answers **reachability**: is there a path from one node to another? Flood from
the source and check whether the destination got painted. Nodes `0` and `2` share a component,
so a path exists (`0`); `0` and `3` do not, so none does:

```tool eval (gp_reach (gp_undir0 0) 0 3)
```

Flood fill is the undirected workhorse the way in-degree is the directed one — components,
reachability, island counting, and "are these two things connected?" are all the same sweep.

## 3. Colouring — the bipartite test

A graph is **bipartite** if its nodes split into two groups with every edge crossing between
them — equivalently, if it is **2-colourable**. The test is a BFS that colours a start node,
gives its neighbours the opposite colour, their neighbours the original colour, and so on. If
this ever tries to give a node a colour it already has the *opposite* of, two adjacent nodes
share a colour and the attempt fails.

The deep fact underneath is a theorem: **a graph is bipartite if and only if it contains no
odd-length cycle.** Walk around any cycle alternating colours; you return to the start with the
start's colour only if the cycle had even length. So an even cycle 2-colours cleanly while an
odd one cannot. The 4-cycle `0-1-2-3-0` is bipartite:

```tool eval (gp_bipartite (gp_bip0 0))
```

…but the triangle `0-1-2-0`, an odd cycle, is not — colour `0` red, `1` blue, and `2` must be
both:

```tool eval (gp_bipartite (gp_odd0 0))
```

(`0` = bipartite, `1` = not.) Recognising "can this be split into two conflict-free groups?" as
a 2-colouring — and that it works exactly when there is no odd cycle — turns a vague phrasing
into a five-line BFS.

---

Graphs join the rest of the toolkit — paradigms, data-structure patterns, weighted graphs,
number theory, bit manipulation, strings, grids, design, trees, dynamic programming, intervals,
and binary search. Every arm here is in `lib/graphs.lat` (prefix `gp_`), and
`latte graphs <topic>` prints these in a terminal (topics: ordering, components, bipartite).
