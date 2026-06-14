# Accountable planning — a democratic plan, computed

`lib/acplan.lat` implements the algorithmic core of *Materialist Economics, Part 3:
Accountable Planning* (Snapshots of the Labyrinth, 2026,
<https://snapshotsofthelabyrinth.photo.blog/2026/03/21/materialist-economics-part-3-accountable-planning/>).
Where the `plan` tool (see `planning.md`) computes a plan from a final demand handed
to it, accountable planning asks **where the demand comes from** and **who is held
to account for meeting it**. The people vote on what to produce; the vote becomes the
final-demand vector; the same Cockshott–Cottrell input-output computation that `plan`
uses turns it into balanced production targets and labour values; production is then
costed and remunerated in labour time, and audited against the plan.

The whole pipeline:

```
quadratic vote  ─▶  final-demand vector  ─▶  input-output computation
                ─▶  balanced targets + labour values
                ─▶  labour-time cost ledger + labour vouchers
                ─▶  accountability check (are the demanded goods produced?)
```

`acplan` is a builtin module: its arms are in scope directly (`latte eval '(qv_cost …)'`),
and `latte ddia acplan` runs the guided tour. It is self-contained — it reimplements the
small fixed-point linear algebra it needs under its own `ap_` names rather than importing
`plan`, so it composes into the global scope without clashing with the numeric libraries.

Everything is **fixed point, scaled by 1000**: 0.4 is stored as `400`, 12 units as `12000`.
Quadratic-voting credits and vote counts are the exception — they are plain integers.

## 1. Quadratic voting → the final-demand vector

Every citizen gets an equal budget of vote credits and spreads votes across goods
categories. Casting *n* votes on one category costs *n²* credits (`qv_cost`). The
quadratic price is the whole point: it makes the citizen internalise the cost of their
influence, so a ballot reveals **how much** each good is wanted, not merely **whether**
it is wanted — and because concentrating votes is dear, an intense minority can prevail
in the one category it needs most while no bloc can cheaply sweep every category
(Lalley & Weyl 2018).

```
(qv_cost [0 [0 [2 [4 0]]]])                  :: 2² + 4² = 20 credits
(qv_afford 30 [0 [0 [2 [4 0]]]])             :: within a 30-credit budget?  0 = yes
(qv_afford 15 [0 [0 [2 [4 0]]]])             :: within 15?  1 = no
(qv_marginal 4)                              :: marginal price of the 4th vote = 2·4-1 = 7
```

Aggregating every citizen's ballot gives society's total votes per category — the raw
popular will — which `ap_demand` reads as quantities of finished goods (scaling into the
planner's fixed point):

```
(qv_tally (ap_ballots 0))                    :: three citizens -> [0 0 6 12] votes
(ap_demand (qv_tally (ap_ballots 0)))        :: final demand [0 0 6000 12000]  (6 corn, 12 bread)
```

## 2. The input-output computation

A `technology` is a list of recipes; recipe *j* is column *j* of the input matrix **A** —
the amounts of each good consumed to make one unit of good *j*. `ap_labourvec` is the
direct labour per unit. The sample is a four-good iron/coal/corn/bread economy (`ap_tech 0`):
iron takes 0.2 coal; coal takes 0.1 iron; corn takes 0.05 iron, 0.1 coal, 0.2 corn as seed;
bread takes 0.1 coal and 0.5 corn.

```
(ap_labourvec 0)                             :: direct labour: iron 1.0 coal 0.5 corn 0.8 bread 0.3
```

**Balanced production targets** solve `x = y + A·x` — to deliver final demand *y*,
production must also cover everything consumed making it. **Labour values** solve
`v = l + v·A` — the labour, direct and indirect, embodied in one unit. Both are the same
*contractive affine transform*: feed an estimate back through the production table and the
error shrinks each pass, converging in ~20 iterations (the "turnpike"). The residual
`x − (y + A·x)` is the convergence gauge — zero at the fixed point.

```
(ap_targetsof (ap_sample 0))                 :: targets [1041 2908 15000 12000]  (×1000)
(ap_residual (ap_tech 0) (ap_targetsof (ap_sample 0)) (ap_demandof (ap_sample 0)))   :: 0 = converged
(ap_valuesof (ap_sample 0))                  :: labour values [1122 612 1147 935]
(ap_labourof (ap_sample 0))                  :: total social labour the plan needs (Σ l·x)
```

Bread is a pure consumption good, so its target (12000) equals the vote; corn comes out
at 15 units — the 6 voted for, plus 6 consumed as an input to the 12 bread, plus 3 of
seed corn — while iron and coal appear as the intermediate goods nobody voted for but the
plan still needs. `ap_plan` bundles the four results; `ap_demandof` / `ap_targetsof` /
`ap_valuesof` / `ap_labourof` pull them out.

When the labour budget cannot meet every target, `ap_allocate` rations it to maximise
social harmony `Σ 1−(1−r)²` over the goods (the proposal's "necessary, not total"
triage), spending the first hours where they do the most good:

```
(ap_costs (ap_valuesof (ap_sample 0)) (ap_demandof (ap_sample 0)))               :: labour to fully meet each demand
(ap_allocate (ap_valuesof (ap_sample 0)) (ap_demandof (ap_sample 0)) 8000 60)    :: ration 8 hours of 18
```

## 3. Intermediate production at labour-time cost

Projects needing the same input pool support for a shared facility, billed at the labour
embodied in what it delivers — direct labour plus the indirect labour in its own inputs —
with **no profit margin**. A delivery debits the consumer's labour budget and credits the
producer; the ledger is a running account, not circulating tokens.

```
(ap_embodied (ap_valuesof (ap_sample 0)) 2 5000)                                 :: labour in 5 corn = 5735
(led_bal (ap_deliver (vou_issue 0 %housing 10000) %housing %works (ap_valuesof (ap_sample 0)) 2 5000) %housing)   :: 10000 − 5735 = 4265
(led_bal (ap_deliver (vou_issue 0 %housing 10000) %housing %works (ap_valuesof (ap_sample 0)) 2 5000) %works)     :: producer credited 5735
(ap_cheaper 1100 1147)                        :: switch to the cheaper facility?  0 = yes, A wins
```

The debit and credit always sum to the consumer's original budget — labour is conserved,
because nothing is added but the labour actually performed.

## 4. Labour vouchers and the price mechanism

Workers are paid in labour vouchers. The defining property (Marx, *Critique of the Gotha
Programme*, 1875) is that vouchers **do not circulate**: a voucher records the labour a
worker contributed, is redeemed once for goods, and is cancelled. It cannot be lent,
invested, or accumulated — which is exactly why there is **no `vou_transfer` arm**. That
absence is the design: it breaks the M–C–M′ circuit through which money becomes capital.

```
(vou_held (vou_redeem (vou_issue 0 %ann 8000) %ann 3000) %ann)       :: issue 8h, redeem 3h -> 5h held
(vou_total (vou_issue (vou_issue 0 %ann 8000) %bo 6000))             :: total outstanding = 14h
```

The **MELT** (Monetary Expression of Labour-Time) is money per labour hour; a good's
predicted price is its labour value times the MELT — the empirically stable
labour-value/price regularity the proposal rests on:

```
(ap_melt 36400000 18095000)                                          :: money ÷ labour ≈ 2.011 per hour
(ap_prices (ap_valuesof (ap_sample 0)) (ap_melt 36400000 18095000))  :: prices [2256 1230 2306 1880]
```

## 5. Accountability

The planners are accountable for delivering the plan. Per good, the shortfall is how far
production falls below target; a good still short by more than a statutory margin after
the subsidised period is the legal trigger for assigning or expropriating the means of
production to meet the demand the people voted for.

```
(ap_shortfalls (ap_targetsof (ap_sample 0)) [1041 [2908 [15000 [10000 0]]]])         :: bread short by 2000
(ap_met (ap_targetsof (ap_sample 0)) (ap_targetsof (ap_sample 0)))                   :: all met?  0 = yes
(ap_undersupplied (ap_targetsof (ap_sample 0)) [1041 [2908 [15000 [10000 0]]]] 1000) :: under-supplied goods -> [3] (bread)
```

## Arm reference, by section of the proposal

| Proposal section | Arms |
| --- | --- |
| §2 Democratic job creation (quadratic voting) | `qv_cost` `qv_afford` `qv_marginal` `qv_tally` `ap_demand` |
| §3 Accountable planning (input-output) | `ap_targets` `ap_values` `ap_anorm` `ap_residual` `ap_labour` `ap_plan` `ap_demandof` `ap_targetsof` `ap_valuesof` `ap_labourof` |
| §3 Triage under a labour budget | `ap_costs` `ap_allocate` |
| §6 Intermediate production at cost | `ap_embodied` `led_new` `led_bal` `led_transfer` `ap_deliver` `ap_cheaper` |
| §5 Labour vouchers + price | `ap_melt` `ap_prices` `vou_new` `vou_held` `vou_issue` `vou_redeem` `vou_total` |
| §3/§7/§8 Accountability | `ap_short1` `ap_shortfalls` `ap_met` `ap_undersupplied` |
| fixtures | `ap_tech` `ap_labourvec` `ap_ballots` `ap_sample` |
| internal fixed-point algebra | `ap_dot` `ap_vadd` `ap_matvec` `ap_transpose` (+ harmony helpers) |

## What this models, and what it does not

This is the *computational skeleton* of the proposal — the parts that are algorithms —
not the constitutional, legal, and political architecture the essay spends most of its
length on (entrenched human rights, a deliberately fragmented government, due-process
expropriation, the tolerated "capitalist remainder"). Honest simplifications:

- **Fixed-point rounding.** Scale 1000 means coefficients and outputs are rounded to the
  nearest thousandth each pass. The identity `Σ v·y = Σ l·x` therefore holds only
  approximately — for the sample, 18.10 vs 18.10 hours, drifting in the third decimal
  (about 0.04%). Exact-rational arithmetic would close the gap.
- **Quadratic voting** is implemented as the cardinal cost mechanism (`n²` credits,
  intensity, minority protection). The deeper strategyproofness and Pareto-efficiency
  results (Lalley & Weyl 2018; Weyl 2017 on collusion) are properties of the mechanism,
  not extra code.
- **Whole-economy, single technology.** One technology matrix and one labour vector stand
  for the economy; there is no regional or temporal disaggregation, and a fixed iteration
  count stands in for a convergence test (the residual arm lets you check it).
- **The ledger and vouchers are accounting models** — they enforce conservation and
  non-circulation by construction (debits underflow loudly; there is no transfer arm), but
  they do not model identity, fraud, or the distributed infrastructure §2 calls for.

## The text-frame front end (Acplan.Tool)

In the System GUI (`latte serve`, open `/`), the **Acplan** header link opens
`Acplan.Tool` — an editable command sheet whose buttons run the planner and embed
a live dashboard wherever you click. The dashboard is itself a Latte arm: `ap_dash`
builds an HTML table (`[%html cord]`) of each good's final demand, balanced target,
labour value, and price, with the total social labour, MELT, and turnpike residual
underneath; the host marks any `[%html …]` result so the GUI embeds it as an object.

The sheet is a few controls over those arms:

- **[The three-citizen sample plan]** runs `acplan.ap_dash0` — the built-in ballots.
- A **vote → plan** row has `corn`, `bread`, `iters`, and `money` fields feeding
  `acplan.ap_dash (ap_quickvote $corn $bread) $iters $money`: the two vote counts
  become a single aggregate ballot, and the money supply sets the MELT.
- An **accountability audit** row has the produced amounts of each good feeding
  `acplan.ap_audit4 $piron $pcoal $pcorn $pbread`, which flags every good under its
  target (the collectivisation trigger), rendered by `ap_audit`/`ap_auditrow`.
- A full-electorate example line passes three custom ballots as a list literal,
  since `[field: …]` values cannot contain `]`.

Everything is customizable in place: edit a field and click, edit a command line
and middle-click it, or edit the `ap_dash`/`ap_audit` arms (open `acplan` in the
Modules list), **Compile**, and run again. **Store** writes your version to
`text/acplan-tool.md`, which then overrides the built-in sheet on the next run
(delete that file to restore the default). The arms live in `lib/acplan.lat`:
`ap_dash` / `ap_dash0` / `ap_quickvote` (the dashboard), `ap_audit` / `ap_audit4`
(the audit), and `ap_th` / `ap_trow` / `ap_rows` / `ap_goodnames` (the table
builders), all returning tagged HTML cords built with the `std` cord toolkit
(`catall`, `numtext`, `fixtext`).

## Sources

- *Materialist Economics, Part 3: Accountable Planning* (2026) — the proposal.
- W. P. Cockshott & A. Cottrell, *Towards a New Socialism* (1993) — the iterative
  labour-value and gross-output computation (`plan.lat`; see `planning.md`).
- S. Lalley & E. G. Weyl, "Quadratic Voting: How Mechanism Design Can Radicalize
  Democracy" (2018) — the voting mechanism.
- K. Marx, *Critique of the Gotha Programme* (1875) — non-circulating labour vouchers.
