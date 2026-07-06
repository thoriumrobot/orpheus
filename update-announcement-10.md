# Orpheus update — collaborative planning and collaborative code

## The shared documents grew kinds

A document in the collaborative editor now declares what its converged text
is *for*: **note** (prose), **plan** (an economy spec), **votes** (a
quadratic-ballot sheet), or **code** (a live Latte module). The kind rides
the id prefix on the one notes agent — same blocks, same anchors, same
tombstones, same convergence guarantees — and the editor shows a matching
action panel: the shared document, put to work.

## Collaborative economic planning

A plan document holds the Cockshott–Cottrell spec one line per block —
`sector steel l=0.4 steel=0.2`, `demand steel=1.0` — edited concurrently by
every connected instance: two people own two sectors, blocks merge by the
usual rules, and `Plan.solve` runs the full planner (labour values, gross
outputs, market steering — lib/plan.lat, all on Loom) on the CONVERGED
spec. Verified live: instance A contributed steel, instance B contributed
grain and the demand line, B solved the whole.

## Collaborative accountable planning

The accountable planner (lib/acplan.lat) was built for many voices; now the
voices arrive over the network. A votes document is a ballot sheet — one
quadratic ballot per participant per line, `ada: 0 0 2 4` over
iron·coal·corn·bread, n votes costing n² credits so intensity is priced —
and each participant adds *their* line from *their* instance.
`Acplan.solve` computes vote → final demand → Leontief targets → labour
values → voucher prices from the whole converged electorate, and the budget
binds: a 37-credit ballot is refused by name. In the live test the demand
column was literally the tallied votes from two instances (corn 2+3=5,
bread 4+3=7). Planning that originates with the people, gathered by gossip.

## Collaborative code

A code document is a shared Latte module — `core name`, one arm per block,
several editors. `Code.check` compiles the converged source (reporting the
module's own arm count); `Code.run` evaluates an expression with its arms in
scope; and `Code.load` is **the Oberon move on a shared document**: compile
the converged module and load it live into the running system, arms callable
from the console, pages, and other code. Loading is per-instance and
explicit — editing never executes anything; a person decides when their
instance adopts the code, and the docs say plainly that shared source
deserves reading first. Verified live: one instance wrote the arms, the
other checked, ran `(ninth 5) = 45`, loaded, and called `(triple 14) = 42`
from the generic evaluator.

## One editor, one registry, as always

The /notes editor gained the kind selector, per-kind hints, monospace for
the structured kinds, and the action panels (⚙ Solve plan · ⚖ Accountable
plan · ▶ Run / ✓ Check / ⟲ Load) — all thin calls into the same tool
registry (`Plan.solve`, `Acplan.solve`, `Code.check/load/run`,
`Note.create(title, kind)`), so everything works identically from the
tools page and the System console. Tests cover the whole loop: spec blocks
→ planner report, converged ballots → accountable plan with the quadratic
budget enforced, and a shared module compiled, loaded, and computing.

## And the survey round: collaborative conlanging

A look across the remaining modules found one more natural fit: **SCArs**.
A language document (kind s) carries a conlang whole — sound-change rules
as blocks (document order IS rule order), lexicon entries alongside — so
one person refines the phonology while another grows the vocabulary, each
from their own instance. `Lang.evolve` runs the converged rules over the
converged lexicon; `Lang.trace` derives a word step by step; tombstoning a
rule block changes the language, and the history slider replays its whole
evolution. Verified live across two instances (A wrote `k > g / _a` and
`a > o`, B added `kasa house` and `taka roof`; both saw kasa → goso).
The drawing editor was surveyed and deliberately deferred with a written
design; the market lab and the already-networked chess/Board were ruled
out on their merits — the reasoning is in docs/collaborative-notes.md.
