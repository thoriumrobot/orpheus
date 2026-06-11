# Economic planning — Towards a New Socialism, computed

The planning tool implements the computational core of W. Paul Cockshott & Allin
Cottrell's *Towards a New Socialism* (Spokesman, 1993): labour-value accounting,
input-output planning, consumer-goods market clearing, and harmony-function plan
balancing. Every numeric step runs in `lib/plan.lat` on Loom; the host
(`src/plan.rs`) only parses specs and narrates results.

Run it:

```
latte plan                      # the classic two-sector worked example
latte plan --demo3              # 3 sectors + market steering + a binding labour budget
latte plan --spec myeconomy.txt # YOUR economy (format below)
```

In the System GUI, `plan demo3` embeds the report wherever you run it; the `/plan`
page has a spec form.

## The economy spec

```
sector steel  l=0.4  steel=0.2 grain=0.1    # inputs per unit output + direct labour
sector grain  l=0.3  steel=0.5 grain=0.1
sector bread  l=0.2  grain=0.6
demand steel=0 grain=1.0 bread=2.0          # net final demand
market steel=0.9 grain=1.6 bread=0.8        # observed clearing prices (optional)
labour 1.2                                  # the labour budget (optional)
```

## What gets computed

**Labour values** — `v = l + v·A`, iterated (`values` in plan.lat). The recurrence
converges from below for any productive (Hawkins-Simon) economy; v_i is the total
labour embodied in one unit of good i, direct and indirect. This is the book's
unit of account: prices, wages, and the plan itself are denominated in hours.

**Gross outputs** — `x = y + A·x`, iterated (`gross`). To deliver the net demand y,
production must also cover its own intermediate consumption; x is the Leontief
solution computed by the same fixed-point style of recurrence the book proposes
for sparse national tables.

**Labour-token accounting** — the plan's cost is Σ v·y hours of social labour;
workers are credited tokens for hours worked and the consumer-goods plan is what
those tokens buy (TNS ch. 2, 5). Tokens do not circulate: they cancel on use.

**Market clearing steers the plan** (TNS ch. 8) — consumer goods sell at whatever
token price clears the market. The ratio of clearing price to labour value is the
plan's feedback signal: p/v > 1 means consumers value the good above its labour
cost, so its target expands; p/v < 1 contracts it. The tool applies one steering
step (ratios clamped to [½, 2] per period) and re-plans gross outputs. This is
the book's answer to "planning ignores preferences": preferences arrive as
clearing prices, and the plan chases them.

**Harmony balancing** (TNS pp. 94-99) — when the labour budget cannot meet every
target, the tool allocates labour to maximize total harmony Σ h(r_i), with
h(r) = 1 − (1−r)² over fulfilment ratios r_i: concave, so the first units toward
any target matter most. The optimum equalizes marginal harmony per labour hour;
the algorithm transfers parcels from the lowest marginal to the highest until they
meet — Cockshott's marginalist method, with the harmony function standing as a
social utility function. (Cockshott reports the full algorithm as O(N log N) in
the complexity of the economy, which is the book's computational-tractability
claim against the older "millions of equations" objection.)

## Criticisms, stated honestly

A tool that implements a contested proposal owes the reader the contest.

**The Hayek information argument.** Hayek (1945) argued the knowledge planning
needs — local, tacit, dispersed, often non-numerical — cannot be centralized;
prices are the compression that makes the dispersed knowledge usable. Cockshott &
Cottrell's reply is partly technical (modern telecoms and computing make gathering
technical coefficients feasible where 1930s calculation was not) and partly
structural (their consumer-goods market is precisely a channel for dispersed
preference information; the plan does not pretend to know demand a priori, it
*measures* clearing prices). What the reply does not dissolve: technical
coefficients are reported by enterprises with incentives to misreport (padding
input requirements was endemic in Soviet practice), and innovation — new goods,
new techniques — has no obvious representation in a fixed I-O matrix. The tool
shares these limits: its A matrix is whatever you type in.

**"These are just Jacobi solvers."** Cosma Shalizi's 2012 critique observed that
the TNS algorithms are linear-equation iterations of a standard sort, and that the
hard part of planning is not solving the system but the optimization and the data.
Cockshott's reply (2018) accepts the mathematical kinship and counters that the
harmony method is deliberately modelled on the neoclassical reallocation story —
the point was never numerical novelty but feasibility at national scale. Both can
be right: the iterations here ARE simple (you can read them in plan.lat in twenty
lines), and the claim being tested is tractability, not cleverness.

**Scale and nonlinearity.** Real economies have millions of products, joint
production, increasing returns, and capacity constraints that a linear I-O model
ignores. The book's answer is sparsity (each product has few direct inputs) and
the N log N harmony pass. This tool demonstrates the algorithms' logic on
economies you can type, not their performance at national scale.

**Incentives and politics.** Labour tokens and plan targets do not by themselves
answer who audits the auditors — TNS pairs the economics with sortition-based
democracy precisely because the authors do not think the algorithm alone settles
the political question. The tool computes; it does not adjudicate that debate.

## References

- Cockshott, W.P. & Cottrell, A., *Towards a New Socialism*, Spokesman, 1993
  (esp. ch. 2, 5, 6, 8; harmony algorithm pp. 94-99; I-O iteration pp. 84-89).
- Hayek, F.A., "The Use of Knowledge in Society", *AER* 35(4), 1945.
- Shalizi, C., "In Soviet Union, Optimization Problem Solves You", 2012.
- Cockshott, P., "Plan balancing algorithms — reply to some criticisms", blog, 2018.
- Cockshott & Cottrell, "Mises, Kantorovich and Economic Computation" (MPRA 6063).
