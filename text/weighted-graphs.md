# Weighted graphs — shortest paths & spanning trees

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/wgraph.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Plain BFS and DFS (in the *algorithm-techniques* tour) treat every edge as one step. Real
graphs have **weights** — distances, costs, times — and two questions dominate
interviews:

- **"cheapest route from A to B?"** → **Dijkstra** (shortest path)
- **"cheapest way to connect everything?"** → a **minimum spanning tree** (Kruskal or Prim)

Every example uses one small undirected graph, weights on the edges:

```
        4
    a ------- b
    |  \      | \
   1|   \2    |5 \
    |    \    |   \
    c ----+   d    (b–d 5)
     \   2|   |
    8 \   |   |3
       \  |   e   (d–e 3)
        \ |
         (c–d 8)
```

Concretely: `a–b 4`, `a–c 1`, `b–c 2`, `b–d 5`, `c–d 8`, `d–e 3`. The graph is given as an
adjacency map (`wg_graph0`) for Dijkstra and Prim, and as an edge list (`wg_edges0`) for
Kruskal.

## 1. Dijkstra — cheapest route from a source

**Cue:** shortest/cheapest path, single source, **non-negative** weights. **Idea:** keep a
tentative distance to every node; repeatedly **settle** the closest unsettled node (its
distance is now final, since any other route would pass through something already at least
as far) and **relax** its edges — offering neighbours a cheaper route through it.

Here are the shortest distances from `a` to everywhere at once:

```tool eval (wg_dijkstra (wg_graph0 0) (wg_nodes0 0) %a)
```

The instructive entry is `b`. The direct edge `a–b` costs 4, but going `a→c→b` costs
`1 + 2 = 3`, so the shortest distance to `b` is **3** — Dijkstra finds the detour:

```tool eval (wg_distto (wg_graph0 0) (wg_nodes0 0) %a %b)
```

And the farthest node, `e`, is reached by `a→c→b→d→e` = `1 + 2 + 5 + 3 = 11`:

```tool eval (wg_distto (wg_graph0 0) (wg_nodes0 0) %a %e)
```

(One catch worth saying aloud in an interview: Dijkstra assumes weights are non-negative.
With negative edges you need Bellman–Ford instead. Latte's atoms are unsigned, so the
non-negative case is exactly what we have.)

**Why settling is safe — the invariant.** When Dijkstra picks the closest unsettled node `u`
and declares its tentative distance final, the justification is an *exchange argument*: any
alternative path to `u` has to leave the already-settled region somewhere, and that crossing
node is by definition unsettled, hence at least as far as `u`; the remainder of the path only
adds non-negative weight, so it can never beat the route we already hold. This is exactly
where non-negativity earns its keep — a negative edge appearing later could undercut a node
we have already frozen, which is precisely why Dijkstra is *wrong* on negative weights rather
than merely slow. Complexity: the array version here is O(V²) (a linear scan to find the next
node, V times); swapping in a binary heap makes it O(E log V), better on sparse graphs.

## 2. Minimum spanning tree — connect everything for the least

**Cue:** "connect all the nodes", "lay cable/road to every site", "least total cost to make
it one network". A spanning tree touches every node with no cycles; the *minimum* one has
the least total edge weight. Two greedy algorithms both find it.

**The theorem underneath both — the cut property.** Partition the nodes into any two
non-empty groups; call that split a *cut*. The single cheapest edge crossing the cut is
*safe*: it belongs to some minimum spanning tree. Every step of both algorithms is an
instance of this one fact. Kruskal's cheapest edge that does not form a cycle is the lightest
edge across the cut separating the two components it would merge; Prim's cheapest edge leaving
the tree is the lightest edge across the cut between the tree and everything else. That is why
two different greedy strategies arrive at the same total — and, more deeply, why greedy works
*at all* here when it fails for so many problems: a local cheapest-across-a-cut choice is
provably part of a global optimum.

### Kruskal — cheapest edges first, skip cycles

**Idea:** sort all edges by weight and walk them cheapest-first, **keeping** an edge only if
its two endpoints are in different components so far — otherwise it would close a cycle.
The "same component?" test is exactly what **union-find** does (reused here from the data
structures tour, `dsa.lat`). Here is the order Kruskal considers the edges:

```tool eval (wg_esort (wg_edges0 0))
```

It keeps `a–c (1)`, `b–c (2)`, `d–e (3)`, then skips `a–b (4)` (a and b are already
connected through c), and finally takes `b–d (5)` to join the two halves — total **11**:

```tool eval (wg_kruskal (wg_nodes0 0) (wg_edges0 0))
```

### Prim — grow one tree outward

**Idea:** start the tree at any node and repeatedly add the **cheapest edge that crosses
out** of the tree, absorbing the node on the other side, until every node is in. Different
route to the answer, same answer:

```tool eval (wg_prim (wg_graph0 0) (wg_nodes0 0))
```

That Kruskal and Prim agree (both **11**) is the point: the minimum spanning tree weight is
a property of the graph, not of the algorithm you reach for.

---

These weighted-graph algorithms sit on top of the plain traversals from the
*algorithm-techniques* tour and the union-find from the *data-structures* tour — the same
parts, recombined. Every arm is in `lib/wgraph.lat` (prefix `wg_`), and `latte wgraph <topic>`
prints these in a terminal (topics: dijkstra, mst).
