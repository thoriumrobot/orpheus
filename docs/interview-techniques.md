# Interview techniques — the coding-technique library

Orpheus ships a collection of **fifteen technique modules**, each a heavily-commented Latte
library covering one recurring category of coding-interview problem (the Toptal / LeetCode
canon). They are written to be *read*: every line carries a comment, every algorithm is paired
with an interactive tutorial that explains *why* it is correct, and every module is reachable
four ways — as a CLI demo, as a GUI tool with live buttons, as an importable library, and as a
tutorial text you can run line by line.

This document is the map. Each module below lists its prefix, what it covers, its headline
arms, the CLI command, and its tutorial. The modules are ordinary builtin libraries (loaded by
default, in scope everywhere — `eval`, the REPL, the GUI console), and every arm also compiles
natively through Anvil (`latte rustc`).

## How to run them

- **CLI demo** — `latte <module> [<topic>]` prints a worked table of examples (e.g.
  `latte dp optimisation`, `latte greedy gas`). Bare `latte <module>` runs every topic.
- **Library** — call any arm directly: `latte eval "(se_isqrt 17)"` → `4`. No `import` needed.
- **GUI tool** — `latte serve`, open `/`, click the header link (Algo, Dsa, … Greedy). Each is
  an editable Oberon-style **tool text** with live buttons and input fields; results embed in
  place.
- **Tutorial** — each module has an interactive tutorial under `text/*-techniques.md`; open it
  in the GUI with `System.OpenText <name>` (or read it here on disk). Every framed command in a
  tutorial is a live object that runs against the real library when the text loads.

## Shared conventions

A few representation choices recur across the modules and are worth stating once:

- **Loobean truth is `0`.** Comparison gates (`lt gt lte gte`) and `==` return `0` for *true*;
  `if c then A else B` takes `A` when `c` is `0`. So predicate-style arms (`is-this-a-BST?`,
  `can-attend-all?`, `contains?`) return `0` for yes, `1` for no.
- **Lists are cons cells terminated by `0`.** A two-element bracket `[a b]` is the *pair*
  `cell(a, b)`; a longer `[a b c 0]` is a proper list. Several modules lean on this: an interval
  is the pair `[start end]`, a tree node is `[value [left right]]` (empty `0`), an assoc entry is
  `[key value]`, and a graph is an assoc list of `[node neighbours]`.
- **`sub` errors on underflow** (atoms are naturals), so every subtraction in these libraries is
  guarded by a prior comparison — a recurring shape worth noticing when reading the code.

The modules also build on the four standard-library additions made for them: a stable
`sort`/`sortby` (intervals, greedy), `bytes`/`frombytes` for cord↔byte-list conversion (strings,
backtracking, palindromes), and the bitwise operators (`bits`). See `docs/latte-language.md`.

## The fifteen modules

### algo — the five algorithm-design paradigms (`al_`)
Skiena's organising scheme. Divide-and-conquer (`al_msort`, `al_bsearch`, `al_qselect`),
dynamic programming (`al_coins`, `al_edit`, `al_lis`), greedy (`al_isort`, `al_sched`),
backtracking (`al_subsets`, `al_perms`, `al_queens`), and graph traversal (`al_bfs`, `al_dfs`,
`al_topo`). Tutorial: `algorithm-techniques.md`. CLI: `latte algo`.

### dsa — interview data structures & coding patterns (`ds_`)
Two pointers (`ds_pairsum`, `ds_ispalin`), sliding window (`ds_maxwin`, `ds_longestuniq`),
fast-and-slow cycle detection (`ds_hascycle`), stacks (`ds_balanced`, `ds_rpn`,
`ds_nextgreater`), prefix sums (`ds_rangesum`), a binary heap (`ds_hinsert`, `ds_heapsort`,
`ds_topk`), a BST (`ds_bstins`), a trie (`ds_tins`, `ds_tprefix`), and union-find (`ds_ufnew`,
`ds_uffind`, `ds_ufunion`, `ds_ufcount`). Tutorial: `dsa-techniques.md`. CLI: `latte dsa`.

### wgraph — weighted-graph algorithms (`wg_`)
Dijkstra shortest paths (`wg_dijkstra`, `wg_distto`), Kruskal (`wg_kruskal`) and Prim
(`wg_prim`) minimum spanning trees. Imports `dsa` for union-find. Tutorial:
`weighted-graphs.md`. CLI: `latte wgraph`.

### numth — number-theory warm-ups (`nt_`)
GCD/LCM (`nt_gcd`, `nt_lcm`), primality and the sieve (`nt_isprime`, `nt_sieve`), modular
exponentiation (`nt_modpow`), factorials/binomials/Fibonacci (`nt_fact`, `nt_choose`,
`nt_fib`), digit operations (`nt_digitsum`, `nt_reverse`). Tutorial: `number-theory.md`. CLI:
`latte numth`.

### bits — bit-manipulation tricks (`bm_`)
Single/missing number via XOR (`bm_single`, `bm_missing`), addition without `+` (`bm_add`),
power-of-two test (`bm_ispow2`), get/set/clear/toggle, subset enumeration over bitmasks
(`bm_subsets`), Hamming distance (`bm_hamming`), population-count DP (`bm_countbits`). Built on
the std bitwise operators. Tutorial: `bit-manipulation.md`. CLI: `latte bits`.

### strings — string algorithms (`st_`)
Reverse and palindrome (`st_rev`, `st_ispalin`), frequency and anagram (`st_freq`,
`st_anagram`), first unique character (`st_firstuniq`), naive and **Rabin-Karp** rolling-hash
search (`st_find`, `st_rabinkarp`), longest common prefix (`st_lcp`), run-length encoding
(`st_rle`). Tutorial: `string-techniques.md`. CLI: `latte strings`.

### grid — matrix / grid algorithms (`gr_`)
Number of islands by flood fill (`gr_islands`), grid path-count DP (`gr_pathcount`), transpose
and rotate (`gr_transpose`, `gr_rotate`), spiral traversal (`gr_spiral`). A grid is a list of
row-lists. Tutorial: `grid-techniques.md`. CLI: `latte grid`.

### design — data-structure design classics (`dz_`)
Min-stack with O(1) minimum (`dz_mspush`, `dz_msmin`), a queue from two stacks (`dz_enq`,
`dz_deq`), and an LRU cache with recency-on-touch (`dz_lruput`, `dz_lruget`, `dz_lrutouch`).
Tutorial: `design-techniques.md`. CLI: `latte design`.

### trees — binary-tree techniques (`tr_`)
A node is `[value [left right]]`, empty is `0`. In/pre/post-order and level-order traversals
(`tr_inorder`, `tr_levelorder`), height and count (`tr_height`, `tr_count`), invert
(`tr_invert`), maximum root-to-leaf path sum (`tr_maxpath`), lowest common ancestor (`tr_lca`),
and validate-BST (`tr_isbst`). Tutorial: `tree-techniques.md`. CLI: `latte trees`.

### dp — dynamic-programming classics (`dp_`)
Climbing stairs and house robber (`dp_stairs`, `dp_rob`), subset-sum and equal partition
(`dp_subsetsum`, `dp_canpartition`), coin-change *ways* (`dp_coinways`), 0/1 knapsack
(`dp_knapsack`), and longest common subsequence (`dp_lcs`). Each is a recurrence over smaller
sub-answers computed once. Tutorial: `dp-techniques.md`. CLI: `latte dp`.

### intervals — interval techniques (`iv_`)
Merge overlapping (`iv_merge`), insert into a sorted disjoint list (`iv_insert`),
can-attend-all and minimum meeting rooms via sweep line (`iv_canattend`, `iv_minrooms`),
two-pointer intersection (`iv_intersect`), total covered length (`iv_coverage`). An interval is
the pair `[start end]`; sorting by start turns overlaps into one sweep. Tutorial:
`interval-techniques.md`. CLI: `latte intervals`.

### search — binary search & its variants (`se_`)
Lower/upper bound and occurrence count (`se_lowerbound`, `se_upperbound`, `se_count`), rotated-
array search and minimum (`se_rotated`, `se_rotmin`), peak finding (`se_peak`), and **binary
search on the answer** — integer square root (`se_isqrt`) and Koko-bananas minimum eating speed
(`se_minspeed`). Tutorial: `search-techniques.md`. CLI: `latte search`.

### graphs — graph decision problems (`gp_`)
A graph is an adjacency assoc list. Kahn topological sort (`gp_toposort`), directed-cycle /
course-schedule detection (`gp_isdag`), connected components and reachability via flood fill
(`gp_components`, `gp_reach`), and bipartite 2-colouring (`gp_bipartite`). Tutorial:
`graph-techniques.md`. CLI: `latte graphs`.

### backtrack — combinatorial search (`bk_`)
Generate valid parentheses (`bk_parens`), combination sum (`bk_combsum`), k-combinations
(`bk_combine`), palindrome partitioning (`bk_partition`), and grid word search (`bk_wordsearch`).
In this pure-functional setting each choice starts its own branch and a step's answer is its
branches concatenated — so there is no explicit "un-choose". Tutorial:
`backtrack-techniques.md`. CLI: `latte backtrack`.

### greedy — greedy algorithms (`gd_`)
Jump game and minimum jumps (`gd_canjump`, `gd_minjumps`), gas-station circuit (`gd_gas`),
partition labels (`gd_partition`), candy distribution (`gd_candy`), and task scheduler
(`gd_taskschedule`). Each is a single linear sweep justified by an exchange argument. Tutorial:
`greedy-techniques.md`. CLI: `latte greedy`.

## Anatomy of a module (and how to add one)

Each technique module is built to the same five-part shape, which doubles as the recipe for
adding your own:

1. **The library** — `lib/<name>.lat`, a `core <name>` block importing `std`, with every arm
   prefixed and every line commented. Test it from source before registering it:
   `latte eval --lib <name>=lib/<name>.lat "(<arm> …)"`.
2. **Registration** — add the `include_str!` constant and a `builtin_lib` match arm in
   `src/latte.rs`, and list the name in `all_libs()` / `builtin_lib_names()`. (See
   `docs/adding-libraries.md` for the general library-registration guide.)
3. **The demo** — a `cmd_<name>` runner in `src/ddia.rs` printing topic-grouped example rows,
   routed from `src/main.rs`.
4. **The tests** — assertions in `src/ddia.rs`'s test module covering each arm and its edge
   cases, run by `cargo test`.
5. **The tutorial and GUI tool** — a `text/<name>-techniques.md` tutorial with live ` ```tool
   eval (…)` fences, plus a tool entry and header link in `lib/site/system.html` and a one-line
   description in `docs/the-system.md`.

After any change to a `.lat` library or the Rust sources, rebuild and refresh the binary
(`cargo build --release && cp target/release/latte ./latte`); the tutorials and GUI tool text
are served from disk and need no rebuild. The whole collection is covered by the unit-test
suite, so a regression in any arm fails the build.
