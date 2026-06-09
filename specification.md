# ORPHEUS — A Complete Specification for a Minimalist Functional Operating Environment

> **Updated edition.** This is the original Phase-1 specification (then titled *Lattice on Quartz*), revised to describe the system **as actually built**. The component names have changed (the project is now **Orpheus** and the language **Latte**; see §0 for the full rename map), and most of what the original plan listed as future work has since been implemented and tested. §10 records everything added since the original specification and the current implementation status of every layer. The original research findings, axiomatic semantics, and design rationale are preserved.

## TL;DR
- **Build the system on a Nock-style tiny interpreted combinator VM** (a twelve-rule deterministic "tree → tree" core) rather than interaction combinators, lambda calculus, or the STG machine: it gives the smallest trusted core, total determinism/reproducibility, trivial serializability, and natural content-addressing — the four properties a distributed, content-addressed, purely-functional system actually needs. Interaction combinators are kept only as an optional parallel accelerator.
- **Replace Hoon's ~100 ASCII digraph runes with a small English keyword set governed by the offside (indentation) rule**, preserving Hoon's terse, regular, AST-transparent feel while curing its central readability pain point; reproduce Oberon's tiling viewer/track/frame UI with clickable `Module.procedure` text-as-command interaction and no application silos.
- **Distribute state with content-addressing + per-node event logs + CRDTs, and no blockchain**: self-certifying key identities, SHA3-256 content hashes (Unison-style), a Merkle DAG store (IPFS-style), and join-semilattice CRDT merge (Automerge/Yjs-style) deliver strong eventual consistency and local-first ownership. The language is named **Latte**, its core VM **Loom**, and the environment **Orpheus** — neutral mathematics/mineralogy names deliberately free of the political baggage that made Urbit's terminology divisive.

## Key Findings

### Oberon — the UI model to carry over
Niklaus Wirth and Jürg Gutknecht's Oberon System (ETH Zurich, 1985–1989; documented in *Project Oberon* and Reiser's *The Oberon System*) partitions the display into **viewers** (windows) made of **frames** (rectangular regions), arranged in non-overlapping vertical **tracks** rather than overlapping windows. Its defining innovation is the **textual user interface**: any text anywhere can contain commands of the form `Module.Procedure`, activated by middle-click ("the command button"); "most Oberon commands take their operands from the screen" (the current selection or marked viewer). There are **no application silos** — as a contemporary review put it, "the notion of an application program completely disappears in favor of groups of small programs called tools." Commands trigger on-demand dynamic module loading; the whole core OS including compiler fit in roughly 131 KB and ~12,000 lines. The philosophy is radical minimalism — Wirth's guiding principle, an adaptation of a line attributed to Einstein, "make things as simple as possible, but not simpler" — yielding (per the 1989 *Software: Practice and Experience* paper) "a system that is a (decimal) order of magnitude smaller than commonly used operating systems." The later A2/Bluebottle descendant kept the click-text-to-execute metaphor but swapped the tiling TUI for a zooming UI.

### Hoon / Nock — the spirit to emulate, the pain to fix
Urbit's **Nock** is, in its own documentation, "a fully axiomatic computing system — no dependencies, no builtins, no hardware dependence — just pure math." Its sole datatype is the **noun**: an atom (arbitrary natural number) or a cell (ordered pair); everything is an acyclic binary tree. Nock 4K has **twelve opcodes (rules 0–11)** and is a deterministic function from noun to noun. Tree addressing uses the **slot operator** `/`: root = 1, head of node *n* = 2*n*, tail = 2*n*+1. **Hoon** compiles to Nock "with no runtime system," is purely functional, statically typed, strictly evaluated, and **subject-oriented**: every expression runs against "the subject," one noun that unifies state, lexical scope, and argument. Its well-known pain point is **runes** — "learning Hoon involves learning nearly 100 ASCII digraph 'runes'," producing what even sympathetic readers describe as "an enormous avalanche of barely structured line noise." Auras (soft atom tags like `@ud`, `@ux`, `@p`), cores (`[battery payload]`), gates, doors, and traps complete the model; **jets** (native reimplementations that must return identical results) provide performance.

### Axiomatic foundation comparison — the central decision
- **Nock-style interpreted combinator VM:** deterministic noun→noun, 12 opcodes, trivially serializable (acyclic trees of naturals, no observable pointer identity), naturally content-addressable. Weakness: naïve interpretation is slow, requiring jets.
- **Interaction combinators** (Yves Lafont, "Interaction Combinators," *Information and Computation* 137(1):69–101, 1997): "a very simple system of interaction combinators, with only three symbols and six rules, is a universal model of distributed computation." Lafont's **Proposition 1** establishes the **strong confluence property** (the one-step diamond): "if a net µ reduces to an irreducible net ν in n steps, then any reduction starting from µ eventually reaches ν in n steps … there is only one possible reduction from µ to ν. So we can say that interaction nets are a deterministic and asynchronous model of computation." Victor Taelin's HVM2 ("HVM2: A Parallel Evaluator for Interaction Combinators") reports it "achieved a near-ideal parallel speedup as a function of cores available (within a single device), scaling from 400 million interactions per second (MIPS) (Apple M3 Max; single thread), to 5,200 MIPS (Apple M3 Max; 16 threads), to 74,000 MIPS (NVIDIA RTX 4090; 32,768 threads)" — but the paper flags these as work-in-progress benchmarks on parallelizable programs only. Critical limitation: the Interaction Calculus uses **affine variables** (each bound variable occurs at most once), so raw self-application — Taelin's README states it is "unable to express others (like λx.(x x))" — is not directly expressible (recursion is recovered via superposition/duplication primitives).
- **Lambda calculi (untyped, System F, Fω, Calculus of Constructions):** confluent (Church–Rosser), with powerful/dependent types, but variable binding, capture-avoiding substitution, and α-conversion add core complexity and complicate serialization.
- **SK/BCKW combinators:** Turing-complete, Church–Rosser confluent, variable-free — but encodings are verbose and human-opaque.
- **Graph reduction (G-machine / Spineless Tagless G-machine):** GHC's proven high-performance lazy model — but it is an implementation technique, not a one-page axiomatic spec.
- **Unison (content-addressed code):** definitions are identified by **SHA3-256** hashes of the syntax tree (named arguments replaced by positional/De Bruijn indices, dependencies by their hashes); mutually-recursive cycles are hashed as a unit, members ordered canonically by their individual hashes. This yields no-build compilation, instant non-breaking renames, perfect caching, and trivial code distribution.

**Verdict:** A Nock-like VM wins on the four criteria that matter here — tiny trusted core, determinism/reproducibility, universal serializability, and content-addressability — while interaction combinators win only on raw parallel throughput and carry an expressiveness caveat plus work-in-progress maturity. We therefore adopt a Nock-style core (**Loom**) as the *canonical* semantics and reserve interaction combinators as an *optional, semantics-preserving* parallel evaluator whose equivalence is guaranteed by Lafont's strong confluence.

### Keywords vs runes
The **offside rule** (Peter Landin, "The Next 700 Programming Languages," 1966) lets indentation delimit blocks, with the lexer inserting virtual braces/semicolons; it is used by Haskell (for `let`/`where`/`do`/`of`), F#, Miranda, and Python. Standard ML, OCaml, Haskell, Elm, and F# collectively demonstrate that a small, regular keyword set plus layout produces terse, readable, easily-parsed functional code. We therefore map each Hoon construct to an English keyword and use layout to eliminate Hoon's punctuation density while preserving its regularity and AST-transparency.

### Distributed shared state — no blockchain
The "local-first" principles of Kleppmann, Wiggins, van Hardenberg & McGranaghan ("Local-first software: you own your data, in spite of the cloud," Onward! 2019, which proposes "seven ideals to strive for in local-first software" — you own your data, works offline, multi-device collaboration, long-term preservation, privacy, user control) combine with **CRDTs** (Automerge/Yjs — JSON-like structures that merge concurrent edits with no central server), **event sourcing** (an append-only log as source of truth, replayed deterministically to derive state), and **Merkle DAGs / content-addressing** (IPFS-style: nodes named by the hash of their contents plus their children's hashes — "self-verified structures," deduplicating and immutable). Together these give a robust, eventually-consistent architecture that is a precise fit for a deterministic functional core, with no consensus protocol and no coin.

---

## 0. Renames since the original specification

The original specification named the system *Lattice on Quartz*. As the system was built, the names were settled and several new components were added. The current names:

| Original name | Current name | What it is |
|---|---|---|
| Lattice on Quartz | **Orpheus** | the whole system / project |
| Lattice | **Latte** | the high-level functional language (source extension `.lat`, binary `latte`) |
| Loom | **Loom** | the axiomatic core VM (unchanged) |
| knot | **Knot** | the sole datatype (atom or pair) |
| Quartz | **Orpheus** (environment) / **Mocha** | the operating environment is the Orpheus GUI; **Mocha** is the application environment that runs inside it |
| — | **Facet** | a markup language for pages (new) |
| — | **Hymn** | the HTTP/1.1 web server (new) |
| — | **Anvil** | the Latte→Rust optimizing compiler (new) |
| — | **Forge** | the collaborative ("team") coding tool (new) |
| — | **SCArs** | the sound-change applier for constructed languages (new) |
| SHA3-512 | **SHA3-256** | the content-address hash actually used |

The mathematical word *lattice* (a partial order with meets and joins, and the join-semilattice algebra of CRDT merge) is retained throughout — only the proper-noun language name changed, to **Latte**, which keeps the coffee-adjacent family with **Mocha**.

# THE FULL SPECIFICATION

## 1. Names and rationale
**Language: Latte.** Named for the mathematical *lattice* — a partial order with meets and joins, exactly the algebra of both a type system and CRDT merge (which is literally a join-semilattice) — softened to **Latte** so it pairs with the **Mocha** application environment. It is short, easy to say, and free of political/ideological association.

**Core VM: Loom.** A loom weaves orderly structure from threads — apt for the tree-weaving reduction engine beneath Latte.

**System & environment: Orpheus.** A neutral, evocative name from myth (the musician whose song ordered the world) for the system as a whole and its operating environment. The Oberon-style environment is delivered as the **Orpheus GUI** (a browser-served console, editor, charts, planner, and live module compiler), with **Mocha** as the application environment that runs inside it.

**Other component names.** The sole datatype is the **Knot** (atom or pair). New surfaces and tools added since the original spec carry their own neutral names: **Facet** (markup), **Hymn** (web server), **Anvil** (the Latte→Rust optimizing compiler), **Forge** (collaborative coding), and **SCArs** (the sound-change applier). All deliberately avoid the moon/feudal/astropolitical naming (galaxies, stars, planets, "the prince") that drew sustained controversy to Urbit, whose creator's political writing made the project "radioactive" in parts of the programming community — a reputational risk this project explicitly designs around.

*(The original specification proposed the mineralogical names Lattice / Loom / Quartz, with alternatives Halite, Strand, Cairn, and Sela; the project kept Loom, renamed Lattice→Latte and Quartz→Orpheus/Mocha, and added the component names above as those parts were built.)*

## 2. Design philosophy and goals
1. **Minimal trusted core.** The complete formal specification of the bottom layer fits on a page, like Nock's: any two conformant implementations either agree or one is provably wrong.
2. **Purely functional, deterministic, total-by-construction at the core.** Same input ⇒ same output, on any machine, forever — the precondition for content-addressing, replay, and distribution.
3. **Readable terseness.** Keep Hoon's regular, compositional, AST-transparent feel; discard its ~100 digraph runes for keywords + layout.
4. **No application silos (Oberon).** Programs are commands (`Module.procedure`) invoked from clickable text in tiled viewers; loading is on demand.
5. **Local-first distribution.** State is owned locally and shared across a personal network by content-addressing + per-node logs + CRDTs; eventually consistent; no blockchain.
6. **One data model everywhere.** Code, data, types, UI, and network messages are the same universal acyclic tree, universally serializable.

## 3. The Axiomatic Core: Loom
Loom, like Nock, is a deterministic total-where-defined function from a tree to a tree.

### 3.1 Data model
A **knot** is the sole datatype:
- an **atom** — an arbitrary-precision natural number, or
- a **pair** — an ordered pair of knots `[a b]`.

Brackets associate right: `[a b c]` = `[a [b c]]`. At the core, atoms carry no type; all higher types live in the Latte layer. Trees are acyclic, pointer identity is never observable, and all structures are persistent/immutable with structural sharing.

### 3.2 Pseudo-operators (spec notation only)
`*` evaluate, `/` slot/address, `#` edit, `?` cell-test, `=` equality, `^` increment. Address rules:
```
/[1 a]        a
/[2 [a b]]    a
/[3 [a b]]    b
/[(n+n) a]    /[2 /[n a]]
/[(n+n+1) a]  /[3 /[n a]]
```

### 3.3 Reduction rules (twelve forms)
Loom mirrors Nock 4K's twelve opcodes (0–11), renamed to mnemonics. Evaluation is `*[subject formula]`:
```
*[s [f g] h]     [*[s f g] *[s h]]                     :: AUTOCONS — distribute
*[s 0 a]         /[a s]                                :: ADDRESS  — fetch subtree a
*[s 1 a]         a                                     :: QUOTE    — constant
*[s 2 f g]       *[*[s f] *[s g]]                      :: EVAL     — compute subject & formula, run
*[s 3 f]         ?*[s f]                               :: CELL?    — 0 if cell, 1 if atom
*[s 4 f]         ^*[s f]                               :: SUCC     — increment
*[s 5 f g]       =[*[s f] *[s g]]                      :: SAME     — 0 if equal else 1
*[s 6 f g h]     if *[s f]=0 then *[s g] else *[s h]   :: IF
*[s 7 f g]       *[*[s f] g]                           :: THEN     — compose (pipe)
*[s 8 f g]       *[[*[s f] s] g]                       :: PUSH     — extend subject (let)
*[s 9 f g]       *[*[s g] 2 [0 1] 0 f]                 :: CALL     — invoke arm f of a core
*[s 10 [a f] g]  #[a *[s f] *[s g]]                    :: EDIT     — replace subtree a
*[s 11 h g]      *[s g]   (hint h evaluated, then discarded) :: HINT
```
The edit operator: `#[a v t]` replaces the subtree at address `a` of `t` with `v`. Rule 11 carries **hints** — discardable metadata a real interpreter may use to (a) dispatch a **jet** (a native fast-path that must return bit-identical results to the pure reduction), (b) attach type/aura info, or (c) emit profiling/trace. As in Nock, dynamic hints must still be computed (they can crash). A formula that reduces to itself is a crash (bottom); a conformant interpreter detects this and yields an out-of-band fault rather than looping.

### 3.4 Why this core (justification)
- **Tiny trusted base:** twelve rules, one datatype — the spec above is the whole semantics, page-sized like Nock's.
- **Determinism & reproducibility:** a pure function tree→tree, identical on every host.
- **Serializability:** every value is an acyclic tree of naturals — trivially marshalled, hashed, and shipped; the precondition for §7.
- **Content-addressability:** any subtree's SHA3-256 hash is a stable global name (§7).
- **Performance path:** increment-only arithmetic is accelerated by jets (rule 11), and whole expressions may be compiled to **interaction combinators** for parallel evaluation — Lafont's strong confluence guarantees the parallel result equals the canonical Loom result — or to native/WASM through a graph-reduction backend.

## 4. The Language: Latte
Latte compiles to Loom with a thin, AST-transparent mapping, exactly as Hoon maps to Nock — but with keywords and layout instead of runes.

### 4.1 Data model and the subject
Latte keeps the **subject-oriented** model: each expression evaluates against one subject tree (state + scope + argument unified). Bindings carry readable names stored in the type, and the surface syntax never forces the programmer to think in raw tree addresses.

### 4.2 Type system
- **Molds** are types-as-functions (a mold is a normalizing/validating function tree→tree); every mold is also a default value ("bunt").
- **Auras** are soft tags on atoms: `nat`, `hex`, `bits`, `byte`, `text` (UTF-8 cord), `char`, `date`, `addr` (node identity), `real` (IEEE float encoded as an atom). Auras nest (`hex` ⊆ `nat`) and may be coerced explicitly; like Hoon's, they are "soft" and not strictly enforced except where cast.
- **Cores** are `[battery payload]` cells: the battery is a tree of code arms, the payload is data/context. **Gates** (one-arm cores with a sample) are functions; **doors** (cores with a sample) are gate-builders / "objects"; **traps** (zero-sample one-arm cores) are loops.
- **Type inference** flows from literals, casts, gate samples, and conditionals; variance (covariant/contravariant/bivariant/invariant — Hoon's gold/iron/lead/zinc "metals") governs core nesting.
- Types form a **lattice**: `meet`/`join` give greatest-lower / least-upper bounds; the bottom type is the never-returning fault.

### 4.3 The keyword set (replacing runes)
Each keyword maps to a Loom construct. Keywords are lowercase English words; blocks use the **offside rule** (a block's children are indented past its keyword; siblings align; a dedent closes the block). An optional explicit `end` terminates variable-arity blocks (cores). Only a tiny, fixed set of irregular symbol forms is retained for high-frequency operations.

| Latte keyword | Replaces (Hoon rune) | Meaning / Loom mapping |
|---|---|---|
| `fn pat -> body` | `\|=` bartis | make a gate (lambda) |
| `let … in` | `=/` tisfas | bind a named value into the subject (PUSH, rule 8) |
| `set face = …` | `=.` tisdot | rebind a face (functional update) |
| `if … then … else` | `?:` wutcol | conditional (IF, rule 6) |
| `case … of` | `?-`/`?+` wuthep | pattern match / switch on a tagged union |
| `is T x` | `?=` wuttis | type/shape test (refines inference) |
| `core … end` | `\|%` barcen | define a core (battery of named arms) |
| `arm name = …` | `++` luslus | a named arm (method) in a core |
| `door … end` | `\|_` barcab | a door (gate-building core with sample) |
| `loop … again(…)` | `\|-` barhep + `$` | a trap (recursion point); re-enter with new values |
| `the T expr` | `^-` kethep | cast: assert `expr` has mold `T` |
| `like e expr` | `^+` ketlus | cast to the type of example `e` |
| `pair a b` | `:-` colhep | construct a cell `[a b]` |
| `list a b c` | `:~` colsig | construct a null-terminated list |
| `tag %foo x` | `[%foo x]` | head-tagged union variant |
| `call f x y` | `%-`/`%+` cen* | apply gate `f` to argument(s) |
| `with ctx expr` | `=>` tisgar | evaluate `expr` with `ctx` as subject |
| `also a expr` | `=<` tisgal | evaluate `expr`, then `a`, sharing subject |
| `use Module` | `/+` faslus | import a module into the subject |
| `raw f` | `!=` zaptis | drop to a raw Loom formula |
| `note "…"` | `::` | documentation/comment |

Irregular symbol forms (the only ones): `[a b]` for a pair, `(f x y)` for application, `a == b` for equality, `+(n)` for increment, and `name(face val)` to evaluate `name` with `face` mutated to `val`.

### 4.4 Evaluation model
Strict (eager), like Hoon. Tail calls in Latte become tail calls in Loom (constant stack). Purely functional: "mutation" of the subject is the functional production of a new subject tree with structural sharing. There are no implicit effects; effects are values (see §4.6 and §7).

### 4.5 Module system
A module is a core; its arms are its exported procedures and types. Modules are **content-addressed**: a module's canonical name is the SHA3-256 hash of its compiled Loom plus interface (§7), with a human-readable alias stored as metadata (Unison-style). Importing (`use`) conses the imported core into the subject — exactly as Oberon's linker "conses together multiple libraries into a tuple." Commands are arms invoked as `Module.procedure` from clickable text (§5). Because names are metadata over immutable hashes, **renames never break callers** and **multiple versions of a module coexist** as distinct hashes.

### 4.6 Worked examples

**(a) List map** (`map`):
```
core list
  arm map = fn [xs=(list a) f=(fn a -> b)] -> (list b)
    loop
      case xs of
        nil          -> nil
        cons x rest  -> pair (f x) again(xs rest)
  end
```

**(b) A small stateful agent** — a counter responding to pokes, effects-as-values:
```
core counter
  state = let count = nat 0

  arm poke = fn [in=action] -> [effects new-state]
    case in of
      %incr    -> pair nil                  count(+(count))
      %reset   -> pair nil                  count(0)
      %report  -> pair (list (tag %told count)) count
  end
```
`poke` is a pure function `(action, state) -> (effects, state')`. The Orpheus runtime applies it to events from the node's log and routes produced effects — i.e. event-sourcing with a deterministic transition function (§6, §7).

**(c) A UI command** — an Oberon-style tool procedure operating on the screen selection:
```
core Edit
  arm open = fn [sel=text] -> view
    let path = (parse-path sel) in
    let doc  = (Files.read path) in
    (Viewer.make path doc)
  end
```
Invoked by middle-clicking the text `Edit.open` after selecting a filename — operands come from the screen (§5).

## 5. The Operating Environment: Orpheus
Orpheus reproduces Oberon's interaction model atop Latte/Loom.

### 5.1 Display model
- The screen is partitioned into non-overlapping **tracks** (vertical columns) subdivided into **viewers**; a viewer is composed of **frames**, and a viewer is itself a frame (recursive, composable). The system provides routines to open, move, and close viewers and to suggest placement — mirroring Oberon directly. An optional **zooming** mode (à la A2/Bluebottle) is a later extension, not the core.
- Every viewer is a value (a core) — a text viewer, a graphic frame, or any user-defined frame type — and is **persistent** across sessions via the single-level store (§6).

### 5.2 Text as command (the heart)
- All text is live. A token of the form `Module.procedure` becomes executable when middle-clicked ("the command button"); right-button selection marks operands.
- Commands take operands **from the screen** — the most recent selection, the marked viewer, or arguments parsed from the tool text following the command — exactly as in Oberon.
- **Tool texts** are editable viewers full of commands: the user's environment is literally a text document of `Module.procedure` invocations they can edit and re-run. There is no shell distinct from the editor.

### 5.3 No applications; composition
There are no applications, only **tools** (cores with command arms). Loading is on demand: invoking `Module.procedure` dynamically loads/links `Module` if not resident, then runs `procedure`. Composition is by piping command output into the subject of the next command (`then`/`with`) and by editing tool texts — yielding Oberon's "finer grain of control" and "modules with multiple entry points."

### 5.4 Module/loading model
The loader is a Loom-level service: given a content hash or alias, it fetches the compiled core (locally or from a peer, §7), verifies the hash, conses it into the subject, and caches it. Because code is content-addressed, loading is reproducible and there are **no builds and no version conflicts** (Unison property).

## 6. Memory, persistence & GC
### 6.1 Runtime memory model
Loom values are immutable trees in a heap of cells with pervasive structural sharing (persistent data structures). Atoms are arbitrary-precision: small atoms are unboxed via tagged pointers, large atoms are boxed bignums. The interpreter is a graph reducer over this heap.

### 6.2 Garbage collection
A **generational copying collector** suits the high-allocation / high-infant-mortality profile of functional graph reduction: a small nursery collected Cheney-style for cheap short-lived cells, with promotion to older generations and occasional full compaction. Because all structures are acyclic and immutable, the collector needs **no cycle detection** and can use simple forwarding; reference counting with structural sharing is a viable alternative for the persistence layer. Oberon, Oberon-2, and A2 all rely on automatic GC — we follow that lineage.

### 6.3 Orthogonal persistence / single-level store
The entire system state is a single Loom tree (the "OS is a noun"). The runtime provides **orthogonal persistence** — state persists transparently with no explicit file I/O — on the model of EROS's persistent single-level store (nodes + pages, checkpoint/migration) and Internet-Computer-style page-map snapshotting with dirty-page tracking:
- The live heap is backed by a memory-mapped store; modified pages are tracked.
- A **snapshot** writes a consistent image periodically; between snapshots, durability comes from the event log (§7).
- Recovery = load latest snapshot, replay the log tail. This is the Urbit-style deterministic single-event-log persistence model, **without** any blockchain or consensus.

## 7. The Distributed Layer
A personal-computer network sharing state, eventually consistent, no blockchain.

### 7.1 Identity and naming
- Each node has a **self-certifying identity**: a public key whose hash is the node's `addr`. No central registry, and **no scarce/feudal address space** — a deliberate departure from Urbit's galaxy/star/planet hierarchy. Human-readable petnames are local metadata.
- All code and immutable data are **content-addressed with SHA3-256** hashes of the canonical compiled tree (following Unison's choice; collision is astronomically improbable). The names→hashes map is separate, mutable metadata. Mutually-recursive definitions are hashed as a cycle, members ordered canonically by individual hash (Unison's scheme), so renames are non-breaking and dependencies are pinned.

### 7.2 Storage: Merkle DAG
Immutable values form a **Merkle DAG** (IPFS-style): each node's content identifier (CID) is the hash of its contents plus its children's CIDs; the structure is self-verifying, deduplicating, and immutable. Loom trees map directly onto this DAG; fetching a value by CID from any peer is integrity-checked by re-hashing.

### 7.3 Replication: log + CRDT
- **Per-node event log (event sourcing):** each node keeps an append-only, content-addressed log of input events; node state is the deterministic replay of that log through the pure transition function (the agent `poke`, §4.6). This yields time-travel debugging, deterministic recovery, and a built-in audit trail — the log is the source of truth.
- **Cross-node shared mutable state uses CRDTs** (state-based or op-based; Automerge/Yjs-style), which merge concurrent edits with no central authority and converge regardless of message order. The algebraic fit is exact: CRDT merge is a **join-semilattice** operation (the recurring lattice theme). Op-based CRDT operations are themselves content-addressed log entries shipped over the DAG.
- **No consensus / blockchain:** there is no global total order and no coin. Convergence follows from CRDT semilattice laws plus causal delivery (vector clocks / a Merkle-clock of log heads), satisfying the local-first ideals (you own your data; offline-first; multi-device).

### 7.4 Consistency guarantees
- **Within a node:** strict serial determinism (single event log).
- **Across nodes:** **strong eventual consistency** — any two nodes that have received the same set of operations are in identical state, regardless of order or duplication (CRDT guarantee).
- **Code/data immutability:** content-addressing makes all shared definitions tamper-evident and permanently resolvable.

### 7.5 Integration with the core
Because Loom is deterministic and all values are serializable acyclic trees: (a) any value can be hashed into the DAG, (b) any computation can be shipped to and reproduced on another node bit-for-bit, and (c) replaying a log yields identical state everywhere. The distributed layer is therefore a thin protocol over the core's intrinsic properties, not a separate runtime.

## 8. Implementation plan
A concrete, staged path from zero to a usable environment, with the empirical thresholds that gate each phase. *(Status tags below reflect the system as built; see §10 for detail.)*

- **Phase 0 — Reference core (Rust). ✅ done.** Implement Loom (12 rules), the knot data model, a tree-walking interpreter, and the canonical hash/serialization format. Deliverable: passes a conformance test-suite; under ~2,000 LOC. *Gate:* fully deterministic; reproducible hashes across machines.
- **Phase 1 — Jets & performance. ✅ done (and exceeded).** Jet dispatch (rule 11) with bignum/arithmetic jets (add/sub/mul/div/mod/lt/dec); and not one but three accelerators beyond the interpreter — an adaptive tiered **JIT**, the **Anvil** Latte→Rust optimizing compiler (now the default engine, with a persistent content-addressed build cache), and an **interaction-net** compiler for an arithmetic/control fragment. All are differential-tested against the interpreter.
- **Phase 2 — Latte compiler. ✅ done.** The front-end (lexer; parser; compile to Loom) plus a standard library, closures/higher-order functions, modules/imports, and a `mold`/`aura` **type checker**. A self-hosting REPL runs Latte defined in Latte. *(Bit-for-bit self-compilation of the whole compiler in Latte remains future work.)*
- **Phase 3 — Persistence + distribution. ✅ done.** Single-level store (append-only event log + snapshots + log-compaction/GC + safe migration by re-fold on agent-CID change); per-node event log; SHA3-256 content-addressing; TCP gossip with Lamport ordering, deterministic fold, and anti-entropy. *Gate met:* nodes converge after independent offline edits.
- **Phase 4 — Orpheus environment. ✅ done via the browser GUI.** Rather than a native viewer/track/frame display server, the environment is delivered as the **Orpheus GUI** served by **Hymn**: a System console, a WYSIWYG **Facet** page editor, charts, the planner, and an Oberon-style live module-compiler page (paste a `core`, compile with line/column errors, load it into the running system). The **Mocha** application environment and **Forge** collaborative coding run on top. *(A native tiling desktop UABI remains future work.)*
- **Phase 5 — Self-hosting environment. ⏳ partial.** Libraries and many tools are written in Latte and hot-loadable into the running system (Oberon-style live recompilation); shrinking the trusted Rust base to just the Loom interpreter + jets + store is ongoing.

**Language/target choices:** **Rust** for the bootstrap interpreter, jets, GC, and store (memory safety, performance, strong WASM story); **C** as an alternate embedding target; **WebAssembly** for portability and sandboxed distribution; an optional **interaction-combinator backend** (HVM-style, compiled to C/CUDA) for embarrassingly-parallel workloads, where Lafont's strong confluence guarantees equivalence to the canonical Loom result.

## 9. Design-decision → research mapping

| Design decision | Justified by research |
|---|---|
| Tiling viewers/tracks/frames; text-as-command `Module.procedure`; no applications; on-demand loading | Oberon System (Wirth & Gutknecht, *Project Oberon* / *The Oberon System*, 1989); A2/Bluebottle |
| Single universal datatype (knot = atom/pair); acyclic trees everywhere | Nock noun model — enables serialization & content-addressing |
| 12-rule deterministic core (Loom) | Nock 4K's twelve opcodes; deterministic noun→noun function |
| Subject-oriented evaluation with named bindings in types | Hoon's subject model, fixed to store readable names |
| Keywords + offside rule instead of ~100 runes | Hoon rune pain point ("nearly 100 ASCII digraph runes"); Landin's offside rule (1966); ML/Haskell/F#/Elm |
| Molds, auras, cores/gates/doors/traps; variance "metals" | Hoon type system, adapted |
| Jets + optional interaction-combinator parallel backend | Urbit jets; Lafont 1997 strong confluence (Proposition 1); HVM2 scaling 400→5,200→74,000 MIPS |
| Nock-style core as canonical semantics over interaction combinators | IC's affine-variable limit (`λx.(x x)` inexpressible) + WIP maturity vs. Nock's proven determinism/serializability |
| SHA3-256 content-addressed code; names as metadata; cycle hashing | Unison codebase-as-database model |
| Merkle DAG storage | IPFS content-addressing / self-verifying DAG |
| Event-sourced per-node log; deterministic replay | Event sourcing / state-machine replication; Urbit single-event-log persistence (minus blockchain) |
| CRDTs for cross-node shared state; no consensus | Kleppmann et al. local-first (Onward! 2019); Automerge/Yjs; CRDT = join-semilattice |
| Orthogonal persistence / single-level store + snapshot | EROS persistent store; Internet-Computer orthogonal persistence / page-map |
| Generational copying GC, no cycle detection | Functional graph-reduction allocation profile; Oberon/A2 GC lineage; acyclic immutable heap |
| Rust bootstrap → self-hosting; native + WASM | STG/graph-reduction practice; Oberon JS emulator precedent |
| Neutral mineral/math naming (Latte/Loom/Orpheus) | Avoiding the political associations of Urbit's terminology (Yarvin controversy) |

## 10. Implementation status and extensions since the original specification

The original specification was a design document. The system has since been built in pure Rust with **zero external crates**, offline, and deterministic, and most of the planned work is implemented and tested. This section records what exists and what was added beyond the original plan.

### 10.1 Core and language
- **Loom** — all twelve rules, `slot`/`edit`/`peg`, `jam`/`cue` serialization, SHA3-256 content-addressing, fuel-bounded crashes. Implemented and conformance-tested.
- **Latte** — full lexer → parser → codegen to Loom. Beyond the original keyword set: a **standard library** (`std`), **closures and higher-order functions** that capture scope, **modules/imports**, `case`/`loop`/`again`/`let`/gates, and a `mold`/`aura` **static type checker** (the `typecheck` tool, e.g. `:type` in the console). A subtle module-layout bug (a right-nested battery whose deepest arm's address grew exponentially and corrupted addressing past ~64 arms) was fixed with a **balanced battery layout**, so modules of any size compile correctly.
- **Jets** — `add`/`sub`/`mul`/`div`/`mod`/`lt`/`dec` as rule-11 native fast-paths that return results identical to the pure reduction (audited).

### 10.2 Execution engines (four, kept in agreement by differential testing)
The original plan called for the interpreter plus jets, with interaction combinators as an "optional accelerator." The built system has **four** execution paths, all checked against the reference interpreter:
1. **Interpreter** — the reference semantics.
2. **Adaptive tiered JIT** — interprets cold code, compiles hot loops to native closures, caches them.
3. **Anvil** — a Latte→Rust **optimizing compiler** (constant folding, native jet lowering, dead-arm elimination, `let`→`let`, tail-call→loop, lambda→closure) that emits a standalone Rust program carrying a tiny self-contained noun runtime. It is the **default engine** for `eval`, the CLI, and the GUI, with a **persistent content-addressed build cache** (keyed by a hash of the generated source; recompiles only when code changes; falls back to the interpreter if `rustc` is absent). Differential-tested over hundreds of expressions including the chess engine and the learned evaluator, on both values and failure modes (overflow, divide-by-zero).
4. **Interaction-net compiler** — Lafont's interaction combinators (γ constructor, δ duplicator, ε eraser) as a confluent reducer, plus a compiler from a fragment of Latte into nets, invokable on real source via `latte net "<expr>"` and narrated by `latte icomb`. The net's primitive agents are naturals under `+`, `*`, `<`, and `if`; on top of those the lowering accepts a richer source fragment by expanding it into the primitives at compile time — `let`, `+(…)`, `==`, **let-bound user functions** (inlined), and `loop … again(…)` as **bounded recursion** (unrolled to a fuel budget). The `if` is **lazy**: the closed condition is evaluated and only the taken branch is built into the net, so the untaken branch is never reduced. This realizes the spec's optional IC accelerator for that fragment; *unbounded* recursion via net-level fixpoints, and a fully dynamic lazy `if` (non-constant conditions, via IC boxes), remain future work (§11). Documented in `docs/interaction-nets.md`.

### 10.3 Persistence and distribution
- **Single-level store** — append-only event log + snapshots + **log compaction/GC**; recovery by replaying the log; **safe migration by re-fold**: because state is a deterministic fold of an agent over the log, changing the agent program never corrupts state (the snapshot records the agent CID, and a mismatch discards the stale cache and re-folds — disarming the Urbit-style "breach").
- **Distributed runtime** — per-node event log, TCP gossip, Lamport total order, deterministic fold, and periodic anti-entropy; nodes converge on byte-identical state and hash regardless of delivery order. Implemented and demonstrated across separate processes.

### 10.4 Surfaces and environment
- **Facet** — a markup language for pages, with conditionals.
- **Hymn** — a hand-written HTTP/1.1 server.
- **Orpheus GUI** — served by Hymn: a System console, a WYSIWYG Facet page editor with Unicode support, charts, the planner, an Oberon-style live module-compiler page, and the **system manuals served in-GUI at `/docs`** (Latte, Facet, SCArs, and interaction nets, rendered from the `docs/` Markdown into a sidebar + pane viewer; reachable from the System page).
- **Mocha** — the application environment; **Forge** — collaborative ("team") coding with attribution.

### 10.5 Latte libraries and applications (written in the language itself)
`std`; `num` (signed fixed-point); `tensor` (n-dimensional); `vec`; `ml` (linear regression by gradient descent); `plot` (SVG); `plan` (a Towards-a-New-Socialism-style economic planner); `chess` (a full engine) and `chessml` (a chess evaluator whose piece values are *learned by gradient descent in Latte*). A board-game tool runs machine players (their own compiled Latte cores) and human-vs-machine play.

### 10.6 SCArs — sound-change applier
A featureful ordered sound-change engine for constructed languages, added after the original specification, shipping with a worked Ligurian "Solar → Heart" derivation and a hosted page. Beyond flat segmental rewrites, the rule language expresses **stress- and cluster-conditioned** changes directly: stress is written with an acute accent (`á é í ó ú`) and multi-segment contexts distinguish open from closed syllables, so `á > a o / _ C V` breaks `kása→kaosa` but leaves the closed `kásta` untouched. A worked ruleset ships as `lib/breaking.sca`, applied with `latte sca --file lib/breaking.sca <word>..`.

### 10.7 Tools / commands
`eval`, `cli`, `repl`, `gui`, `serve`, `node`, `agent`, `selftest`, `bench`, `sca`/`evolve`, `mold`/`typecheck`, `mocha`, `plan`, `team`, `tensor`, `ml`, `chart`, `icomb`, `jit`, `game`, `rustc` (Anvil), `net`, and `cache`.

### 10.8 What remains (frontier)
**Unbounded** general recursion on the interaction-net engine via net-level fixpoints (the net now does `let`, user-defined functions, and *bounded* recursion by unrolling), and a fully *dynamic* lazy `if` for non-constant conditions via interaction-net boxes (the net's `if` is already lazy for the closed conditions it evaluates, building only the taken branch); a native desktop tiling UI (the browser GUI stands in for the Orpheus viewer/track/frame model); and bit-for-bit self-hosting of the whole compiler in Latte. *(The intricate stress- and cluster-conditioned sound changes once listed here are now expressible directly in SCArs rules — see §10.6 and `lib/breaking.sca`.)*

## 11. Caveats
- **A pure interpreted core's performance is unproven at scale** without an extensive jet library. The 400→74,000 MIPS interaction-combinator figures are vendor-reported, explicitly work-in-progress benchmarks on parallelizable programs only, and must not be read as guaranteed end-to-end system throughput.
- **Interaction combinators remain an optional accelerator, not the canonical core**, precisely because they cannot express some lambda terms (affine variables forbid `λx.(x x)`), and the built net compiler covers only an arithmetic/control fragment; equivalence to Loom is re-established by differential testing per compilation.
- **CRDTs guarantee convergence, not application-level invariants.** Some workflows still need explicit conflict surfacing, and CRDT metadata overhead (especially for text) is real.
- **Orthogonal persistence complicates code upgrade/migration:** changing data layouts can invalidate persisted state. Orpheus mitigates this with migration-by-refold (§10.3), but general schema evolution remains a hard problem (the same difficulty noted for Internet-Computer canisters).
- **The original specification was a design document; the system described here has since been built** (pure Rust, zero external crates, deterministic), but the frontier in §10.8 is real, and the Einstein-attributed minimalism motto is, like all aesthetic principles, a guide rather than a guarantee.