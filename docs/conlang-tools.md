# The conlang suite — sound changes, phonologies, and SCArs

Three modular tools that call each other, Oberon-style: the **sound-change
library** assembles attested changes into SCArs rulesets; the **phonology
builder** designs proto-language sound systems and generates vocabulary; and
the **SCArs engine** (lib/sca.lat, on Loom) executes the rules. The chain runs
builder → word generator → change library → engine, and every link is also
usable alone.

## The sound-change library (/soundlib)

Forty-two changes from real language history, each with its provenance: Grimm's
law and Verner's law (Proto-Germanic), Western Romance lenition, Latin
rhotacism, the first Slavic palatalization, i-umlaut, the Great Vowel Shift
fragments, the High German consonant shift, the Canaanite shift, Slavic liquid
metathesis, th-fronting, non-rhoticity, yod coalescence, and more. Entries
state their simplifications honestly (Verner's accent condition is reduced to
its vowel-context core; umlaut acts across one consonant).

Click changes **in order** — chain shifts depend on it (run `midraise` before
`gvs` to feed the shift). The tool assembles a complete SCArs file and runs
your sample words through the engine. In the System: `soundlib grimm1 verner
words=pater bhrater` embeds the before/after table in any text.

## The phonology builder (/phono)

Choose an inventory (presets: polynesian, pie, semitic, sinitic — or type your
own consonants, vowels, and syllable patterns), a word count and seed, and
optionally an ordered set of changes. The generator produces vocabulary obeying
the phonotactics (same seed, same words), then evolves it through the chain.
GUI command: `phono preset=pie n=12 changes=grimm1,verner,apocope`.

### Believability — the typological checks

Every generated phonology is checked against the cross-linguistic record
(Maddieson's UPSID survey; WALS): the implicational universals (voiced stops
imply voiceless counterparts; /z/ implies /s/; front rounded vowels imply both
/i/ and /u/), the near-universals (/t/ and /k/; at least one nasal; a liquid),
and the distributional norms (inventory sizes against the attested range —
Rotokas's 6 consonants at the floor; the a-i-u triangle for 3-vowel systems;
5 vowels as the worldwide mode). Each check reports ok / note / warn **with its
evidence**, and the marked-but-attested cases say so: a Hawaiian-style
inventory without /t/ draws a warning that names Hawaiian as the famous
exception rather than pretending the system is impossible.

## Files other tools can load

- **Rulesets** store as ordinary SCArs files — `sca/<name>.sca` — so anything
  that reads the format loads them directly: `latte sca --file
  sca/germanic.sca pater`, the soundlib page's open menu, or `soundlib
  file=germanic words=…` in any System text. The ordered selection is recorded
  in a `:: changes: grimm1 verner` header, so reopening a stored ruleset
  restores the clickable selection.
- **Phonologies** store as `phonology/<name>.phon`: three plain lines
  (`consonants = …`, `vowels = …`, `patterns = …`) plus `::` comments — a
  format any future tool can parse in a dozen lines. The phono page stores and
  reopens them.

## Ligurian

The original conlang pipeline — PIE → Solar Speech → Heart Speech — lives in
Derive.Tool and the /derive page, on the same SCArs engine
(docs/scars-sound-changes.md documents the rule language; the
breaking.sca example shows stress- and cluster-conditioned rules).
