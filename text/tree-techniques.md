# Binary trees — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/trees.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Trees are the most common shape in interviews, and nearly every tree question is the *same*
idea wearing a different hat: **solve the two subtrees, then combine their answers at the
root.** Height, size, path sums, lowest common ancestor — all of them are that one recursive
template with a different combine step. Recognising it turns a wall of tree problems into
variations on three lines.

Here a node is the triple `[value [left right]]`, and the empty tree is `0` — which doubles
as "no child" and as the base case that every recursion bottoms out on. The running example:

```
        5
      /   \
     3     8
    / \   / \
   2   4 7   9
```

## 1. Traversals — the same walk, visiting the root at different moments

**Depth-first** traversal recurses into the children; *when* it records the root is the only
difference between the three classic orders.

**In-order** = left, root, right. The payoff: on a binary *search* tree this emits the values
in **sorted order**, because everything in the left subtree is smaller and everything in the
right is larger, recursively. That is not a coincidence to memorise — it is the defining
property of a BST made visible:

```tool eval (tr_inorder (tr_tree0 0))
```

**Pre-order** = root, left, right. Because the root is emitted *before* its subtrees, pre-order
is exactly what you need to **serialise** a tree to a list and rebuild it later — you always
know the parent before its children:

```tool eval (tr_preorder (tr_tree0 0))
```

**Post-order** = left, right, root. The root comes *last*, after both children are done, which
is the order you must use to **fold a tree upward** — freeing memory, computing subtree
aggregates, evaluating an expression tree — anything where a parent needs its children's
results first:

```tool eval (tr_postorder (tr_tree0 0))
```

**Level-order** is the odd one out: top-to-bottom, left-to-right, one level at a time.
Recursion cannot produce this naturally, because recursion is depth-first by nature. Instead we
carry an explicit **queue**: dequeue a node, emit it, enqueue its children. Because a queue is
first-in-first-out, nodes leave in the exact order they were discovered — which is breadth-first,
by level:

```tool eval (tr_levelorder (tr_tree0 0))
```

That queue-driven discovery is the same machinery as BFS on a graph (the *algorithm-techniques*
tour); a tree is just a graph that happens to have no cycles.

## 2. Shape — height, size, and the famous invert

**Height** is the template in its purest form: an empty tree has height 0, and any other tree is
`1 + max(height of left, height of right)`. There is nothing to it *but* the combine step — and
almost every "balanced?", "diameter?", "depth?" question is a tweak of this exact recursion:

```tool eval (tr_height (tr_tree0 0))
```

**Inverting** a tree — swap every node's two children, recursively — is the question made famous
as a whiteboard rite of passage. It is three lines, and it is an *involution*: invert twice and
you are back where you started. You can watch it work by inverting and reading in-order, which
comes out reversed:

```tool eval (tr_inorder (tr_invert (tr_tree0 0)))
```

## 3. Paths — maximum root-to-leaf sum

**Cue:** "best path from the root down", "maximum/minimum path". The recursion returns the best
sum *starting at this node*: a leaf's answer is its own value; an internal node adds its value to
the better of its children. The one trap is a node with a single child — descending into the
missing side would score it as a zero-length path and corrupt the max, so a one-child node must
follow only the child it actually has. The best path here is `5 → 8 → 9 = 22`:

```tool eval (tr_maxpath (tr_tree0 0))
```

## 4. Queries — lowest common ancestor, and validating a BST

**Lowest common ancestor** of two nodes is the deepest node that has both in its subtree. The
recursion is beautiful: if the current node *is* one of the targets, return it (a node is an
ancestor of itself); otherwise search both sides. If the two targets come back from **different**
subtrees, their paths first cross *here*, so this node is the LCA. If they come back from the
**same** side, the answer is deeper down that side. `2` and `4` meet at `3`:

```tool eval (tr_lca (tr_tree0 0) 2 4)
```

…while `2` and `7` live in different halves of the tree, so their lowest common ancestor is the
root, `5`:

```tool eval (tr_lca (tr_tree0 0) 2 7)
```

**Validating a BST** reuses the in-order insight from the very first panel. Rather than threading
fragile min/max bounds down the recursion, just take the in-order traversal and check it is
strictly increasing — which is true *exactly* when the tree is a search tree. The sample passes
(`0` = yes):

```tool eval (tr_isbst (tr_tree0 0))
```

…and a tree whose in-order is `2 3 9 5 8` fails the strictly-increasing test, so it is not a BST
(`1` = no):

```tool eval (tr_isbst (tr_bad0 0))
```

---

Binary trees join the rest of the toolkit — paradigms, data-structure patterns, weighted graphs,
number theory, bit manipulation, strings, grids, and design. Every arm here is in
`lib/trees.lat` (prefix `tr_`); a node is `[value [left right]]` and the empty tree is `0`, and
`latte trees <topic>` prints these in a terminal (topics: traversals, shape, paths, queries).
