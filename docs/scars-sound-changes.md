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
