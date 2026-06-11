# Conlang.Tool — the conlanging suite
Four modular tools in one chain: phonology builder → word generator → sound-change
library → SCArs. Full manual: [open it](run: System.Edit conlang-tools)

## The sound-change library (42 attested changes)
words [field: words=pater bhrater lupo skola]
[Grimm + Verner](run: soundlib grimm1 verner words=$words) [Romance lenition chain](run: soundlib lenition apocope finaldevoice words=$words) [English reductions](run: soundlib knloss wrloss ghloss nonrhotic words=$words)

## The phonology builder (+ believability report)
preset [field: preset=pie]  n [field: n=12]  seed [field: seed=42]
[Generate](run: phono preset=$preset n=$n seed=$seed) [Generate + Grimm](run: phono preset=$preset n=$n seed=$seed changes=grimm1,verner,apocope)

## Stored files
Rulesets persist as ordinary SCArs files (sca/*.sca), phonologies as phonology/*.phon —
the /soundlib and /phono pages Store and reopen them; SCArs loads them directly.
soundlib file=germanic words=pater bhrater
