# SCArs — the Sound-Change Rule Language

SCArs ("Sound-Change Rules") is the small declarative language Orpheus uses to model
historical phonology — turning a word in one language stage into its descendant by applying
ordered rewrite rules. The matching engine is written in [Latte](latte-language.md)
(`lib/sca.lat`) and runs on the Loom; the host compiles a rule file into the engine's concrete
rule format. The shipped ruleset, `lib/ligurian.sca`, derives **Heart Speech** (Mazdō-yō
Heardō̃) from **Solar Speech** for the conlang *Ligurian*.

---

## 1. A rule file

A `.sca` file is a sequence of lines. Blank lines and `::` comments are ignored. There are two
kinds of lines: **class definitions** and **rules**.

```
:: a tiny ruleset
class V = a e i o u

p > b / # _        :: word-initial p becomes b
k > g / V _ V      :: k voices to g between vowels
t >   / n _        :: t is deleted after n   (empty right-hand side = deletion)
```

Rules are applied **in order**, each scanning the whole word; the output of one rule is the
input to the next. Ordering matters (a classic chain shift relies on it).

---

## 2. Rule syntax

```
FROM > TO / PRE _ POST
```

- **`>`** separates the target (`FROM`) from its replacement (`TO`).
- **`TO` may be empty** — that deletes the matched segments (`t > / n _`).
- **`/ … _ …`** is an optional context. The `_` marks where `FROM` sits; `PRE` is what must
  precede it and `POST` what must follow. Omit the whole `/ …` for an unconditional change.
  Either side of `_` may be empty (`/ # _` = only at word start).

Each of `FROM`, `TO`, `PRE`, `POST` is a whitespace-separated sequence of **tokens**, so rules
can match and produce multiple segments (`a i > ē`).

### Tokens

| Token | Meaning |
|-------|---------|
| a grapheme | a literal segment, e.g. `p`, `ā`, `kʷ` |
| `#` | a word boundary |
| `*` | a wildcard — any one non-boundary segment |
| a class name | any one member of that class (see below) |

### Classes

`class NAME = m1 m2 …` names a set of graphemes so a rule can range over all of them at once.
A class used in `FROM`/context matches any member; the host expands the rule over the class so
the engine only ever sees concrete segments.

```
class V = a e i o u ā ē ī ō ū
ē > ī            :: unconditional
o > u / _ V      :: o raises before any vowel
```

### Stress- and cluster-conditioned changes

Because contexts are *sequences* of tokens (and classes), the rule language expresses prosodic
conditions, not just flat segmental rewrites. Stress is written with an acute accent on the
vowel (`á é í ó ú` — ordinary graphemes the engine matches); a syllable is **open** when the
stressed vowel is followed by a single consonant and then a vowel. So stressed-vowel **breaking**
in open syllables — and its suppression before a consonant **cluster** (a closed syllable) — is a
single context rule:

```
class V = a e i o u
class C = p t k s n r l
á > a o / _ C V          :: break in an open syllable …
```
```
latte sca kása  --file=…   => kaosa     (open syllable: breaks)
latte sca kásta --file=…   => kásta     (closed syllable, cluster: unchanged)
```

A worked ruleset ships as **`lib/breaking.sca`** (Phase VI breaking plus a cluster-conditioned
onset rule); run a whole `.sca` file with:

```
latte sca --file lib/breaking.sca kása kásta méla spáta
   kása  -->  kaosa
   kásta -->  kásta
   méla  -->  meala
   spáta -->  paota
```

---

## 3. The Ligurian pipeline and `evolve`

`lib/ligurian.sca` encodes the regular, segmental subset of the Solar→Heart changes (Phases
II–V: initial lenition, the vowel drag chain `ō > ū` before `ā > ō`, etc.). The file's header
documents which changes are intentionally *not* segmental.

`evolve` (the host's Ligurian convenience path) does more than apply the file: after the
segmental rules it runs two **prosodic passes** in Rust that need syllable/stress structure a
pure rewriter can't express — stressed-vowel **breaking** (Phase VI) and **nasalisation**
(Phase VII) — and strips the boundary markers. So:

- `evolve(word)` = the full Ligurian derivation (file rules + prosodic passes).
- the general engine = just the rules you give it.

---

## 4. Using SCArs

The same engine is reachable from every layer of the system:

- **CLI, full pipeline:** `latte evolve ligā` → `liɣō`.
- **CLI, arbitrary rules:** `latte sca kasa k>g s>z/a_a` → `gaza`.
- **GUI System console** (`/api/run`):
  - `sca ligā nīvō mazdā` — evolve words through the Ligurian pipeline,
  - `scar kasa k>g s>z/a_a` — the general rule engine on one word.
- **Facet pages:** `{{ SCArs.evolve("zelā") }}` and `{{ SCArs.apply("kasa", "k>g", "s>z/a_a") }}`
  (see [`facet-language.md`](facet-language.md)).
- **Latte:** `import sca` to call the matching engine's arms directly on tokenised words.

---

## 5. Worked examples

```
latte sca kasa  k>g s>z/a_a          => gaza      (intervocalic voicing)
latte sca apataka  p>b/a_a t>d/a_a k>g/a_a   => abadaga
latte sca anta  t>/n_                => ana       (deletion after n)
latte evolve ligā nīvō mazdā         => liɣō  nīvō→…  (full Heart Speech)
```

---

## 6. Notes

- **Order is significant.** A drag chain like `ō > ū` then `ā > ō` must list `ō > ū` first, or
  the original `ō` would be lost to the second rule.
- **`#` is the boundary;** `/ # _` is "word-initial", `/ _ #` is "word-final".
- **Empty `TO` deletes;** there is no separate delete operator.
- **Comments are `::`,** matching Latte — a `.sca` file and a `.lat` file share the same comment
  syntax.
- **Graphemes are whatever you write** (Unicode welcome: `ɣ`, `ā`, `kʷ`); the engine treats
  each whitespace-separated token in a rule as one segment.

---

## 7. Implementation: how much of the SCA runs in Latte

SCArs is being migrated stage by stage out of the Rust host (`src/sca.rs`) and
into Latte, so the sound-change machinery lives in the same hot-loadable language
as everything else. The split today:

- **Matching engine — Latte (`lib/sca.lat`).** The scan-and-splice rewriter
  (`matchseq`/`applyrule`/`applyrules`) that does the actual rule application runs
  on the Loom; the host loads `lib/sca.lat` to build the engine.
- **Tokeniser — Latte (`lib/scatok.lat`).** Grapheme **longest-match**
  tokenisation now runs in Latte too. It works at the byte level but steps by
  whole UTF-8 codepoints, so multibyte letters (`ā`, `ñ`, combining marks) are
  never split, and multi-letter graphemes (digraphs like `th`, `kʷ`) win over
  their prefixes. `(tokenize inventory word)` returns the segment list.
  This is the migration of `src/sca.rs::tokenize`.
- **Rewrite core, end to end — Latte (`lib/scarun.lat`).** `scarun` chains the
  two stages with Latte versions of the host's `word_segments` (a word → its
  boundary-padded segment list) and `seglist_to_string` (segments → cord, with
  the `#` padding dropped), so a word can be rewritten with *no* Rust except the
  rule-string parse: `(run inventory compiled-rules word)`. It is checked
  end to end on intervocalic voicing (`s>z/a_a`: `kasa → kaza`), deletion
  (`t>/n_`: `anta → ana`), and a digraph rewrite (`th>s` with inventory `th`:
  `thath → sas`). The tokeniser is now a **registered builtin** (`import`-free
  `tokenize`); the older all-in-one driver libraries `sca.lat`/`scarun.lat` —
  whose arm names `apply`/`run` are deliberately generic and would clash with
  `chess`/`finbond` in the global set — stay loaded explicitly for development.
  The user-facing entry points are the `latte sca` / `scar` / `evolve` commands in
  §4, which now run the Latte pipeline (see the close of this section).
- **Rule compiler — Latte (`lib/scaparse.lat`).** Parsing a rule *string* into
  concrete tagged tokens — `class NAME = …` declarations, category
  **correspondence** (parallel classes mapping member-for-member, `p t k → b d g`),
  the cartesian expansion of every class choice, wildcard `*`, boundary `#`, and
  `/ PRE _ POST` contexts — now runs in Latte too. `(compile src)` turns a `.sca`
  source into `[rules graphemes]`, and `(apply_src src word)` does the whole job —
  parse, tokenise, apply, stringify — in one call. This required a little new
  Latte text tooling (`splitb`/`words`/`trim`/`before`, an association-list class
  table) since the language had no string-splitting before. It is checked against
  the Rust host for equality on intervocalic voicing (`s>z/a_a`: `kasa → kaza`),
  deletion (`t>/n_`: `anta → ana`), a digraph rewrite (`th>s`: `thath → sas`), and
  the full correspondence rule above (`apataka → abadaga`, `kasta → kasta`,
  `opek → obek` — all identical to `latte sca --file`). To stay loadable beside
  the built-ins without the generic-name clashes (`apply` collides with chess,
  `run` with finbond), `scaparse` carries a private copy of the matching engine
  under `sc_`-prefixed names and imports only `std` and `scatok`; load it with
  `latte eval --lib scaparse=lib/scaparse.lat --lib scatok=lib/scatok.lat
  "(apply_src … )"`.
- **Prosodic passes + full `evolve` — Latte (`lib/scapros.lat`).** The two passes
  that need syllable/stress structure rather than segmental rewriting — Phase VI
  stressed-vowel **breaking** (`a→ao`, `e→ea`, `o→uo` in a stressed open syllable,
  blocked by vowel/`v`/`ɣ` onsets and closed syllables) and Phase VII
  **nasalisation** (a vowel before `m`/`n` + a non-vowel goes nasal and drops the
  nasal consonant; long vowels take a combining tilde) — now run in Latte too,
  over a single-codepoint list obtained with `(tokenize 0 word)`. `(evolve src
  word)` chains the whole thing: strip acute stress, compile + apply the rules,
  then break, nasalise, and drop the `#` padding. It is checked for **equality
  with the host `evolve` on all 28 `ligurian_*` test words** — the 13
  Solar→Heart cases (`ligā→liɣō`, `sālā→hōlō`, `weidā→veiðō`, …) and the 15
  breaking/nasalisation cases (`zelā→zealō`, `bendā→mẽdō`, `kamtón→gãtõ`,
  `ōn→ū̃`, …).

With this the entire SCA — tokenise, parse/compile, apply, break, nasalise,
stringify, i.e. Phases II–VII of the Ligurian pipeline — runs in Latte; nothing of
the sound-change *logic* is Rust-only any more, and the migration is now **wired
into the system rather than loaded for development**:

- `scatok`, `scaparse` and `scapros` are registered **builtin libraries**
  (alongside `std`, `chess`, `finbond`, …), so their arms — `tokenize`,
  `compile`, `apply_src`, `evolve`, … — resolve in `eval`, the REPL, the GUI
  console and the Facet runtime with no `import`. (The one global name clash,
  `nuclei` in `lexis`, was renamed to `vnuclei`.)
- The user-facing **`latte evolve` and `latte sca` commands now run the Latte
  implementation** (`src/sca.rs::evolve_latte` / `run_sca_latte`). Compilation is
  preferred: they build to native via Anvil and fall back to the adaptive engine
  (interpret-cold/JIT-hot) only when native compilation declines. Two notes on the
  native path:
  - The Latte→Rust emitter was extended to **alpha-rename binders that collide with
    Rust keywords** (`as`, `type`, `move`, …) — the `scaparse` arm `zip2 = fn [as
    bs] …` previously emitted `fn arm_zip2(as: V, …)`, which is not valid Rust and
    forced a fallback. With that fixed, the rule engine (`apply_src` and ad-hoc
    `latte sca …` with normal-length cords) now **compiles to native and is
    differential-tested through it**.
  - The full `evolve` / `--file` runs **now compile to native as well**. Cords in
    this nock-like system are base-256 atoms, and the native backend originally
    capped atoms at `u128` (16 bytes), so the ~250-byte Ligurian rule *source*
    passed to `compile` overflowed and forced a fallback. The backend was extended
    to carry long cords as byte-vector atoms (`V::Big`) and to jet the cord ops
    (`bytes`/`frombytes`/`cat`/`catall`/`bytelen`) to byte-vector operations instead
    of base-256 arithmetic — no general bignum required. And rather than baking each
    word into the program (a `rustc` build per word), the backend compiles the whole
    614-rule evolution **once** into a binary that takes the word as a runtime input
    over stdin (`run_native_with_input`, with the word bound to `__in`). So the first
    word pays a one-time ~6 s build and every word after it — sharing that one cached
    binary — runs in ~50 ms, about 5× faster than the adaptive engine (~0.25 s).

The Rust engine and passes (`evolve` / `run_sca` in `src/sca.rs`) are kept as the
**differential oracle** — the `SCArs.apply` Facet path still calls them, and the
test suite asserts the Latte path matches them byte-for-byte. Coverage, all inside
`cargo test` (302 passing): `latte_evolve_matches_host_and_expected` runs all 28
`ligurian_*` words through `evolve_latte` and checks both the expected Heart-Speech
form and equality with the Rust `evolve`; `latte_run_sca_matches_host` does the
same for ad-hoc rules (intervocalic voicing, deletion, digraph longest-match,
ordered multi-rule, class-conditioned deletion, stressed-open-syllable breaking,
category correspondence); `latte_known_outputs` pins `apataka→abadaga`,
`ligā→liɣō`, `zelā→zealō`, `bendā→mẽdō`, `ōn→ū̃`; and
`sca_latte_libs_are_registered_builtins` guards the registration. Each stage thus
carries its own checks — longest-match and multibyte codepoints (`scatok`);
intervocalic voicing, deletion, digraph, correspondence (`scaparse`); breaking and
nasalisation equality (`scapros`).
