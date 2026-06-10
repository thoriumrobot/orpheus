# Data Visualization & Machine-Learning Model Design in Latte

This guide shows how to compute with numbers, draw charts, and design and train
machine-learning models in **Latte** — the functional language that runs on the Loom VM
inside Orpheus. Everything here is pure Latte on top of three small libraries that build on
each other:

```
std   →  num   →  tensor   →  ml      (modelling)
std   →  plot                          (visualization)
```

There are no external dependencies and no floating-point hardware involved: the whole
numeric tower is built from natural numbers and the jetted arithmetic of `lib/std.lat`.

---

## 1. Numbers: signed fixed-point (`lib/num.lat`)

Loom atoms are arbitrary-precision **naturals** — there are no negative numbers and no
fractions. Since both are needed for machine learning (gradients go negative; weights are
fractional), `num` represents a number as a cell `[sign magnitude]`:

- `sign` is `0` for positive, `1` for negative;
- `magnitude` is a **fixed-point** natural scaled by 1000, so `2.5` is stored as `[0 2500]`
  and `-0.75` as `[1 750]`.

The operations mirror ordinary arithmetic:

```
(nadd [0 1000] [0 2000])   :: 1 + 2   = [0 3000]
(nadd [0 1000] [1 3000])   :: 1 + -3  = [1 2000]
(nmul [1 2000] [0 3000])   :: -2 * 3  = [1 6000]
(ndiv [0 6000] [0 2000])   :: 6 / 2   = [0 3000]
(nlt  [1 2000] [0 1000])   :: -2 < 1  -> 0 (true; Loobean 0 = true)
```

The single most important consequence is **precision**: three decimal places. The smallest
representable nonzero value is `0.001` (`[0 1]`). Any intermediate result smaller than that
rounds to zero. This is what eventually limits how finely a model can be trained (§5).

---

## 2. Tensors: n-dimensional arrays (`lib/tensor.lat`)

A tensor is a cell `[shape data]`:

- `shape` is a list of dimension sizes, e.g. `[2 [3 0]]` for a 2×3 matrix;
- `data` is the elements in **row-major** order, each a signed fixed-point number.

Build a 1-D tensor from plain naturals (each becomes `n.0`), then reshape it:

```
(tfromnats [1 [2 [3 [4 [5 [6 0]]]]]])               :: 1-D, six elements
(treshape (tfromnats [1 [2 [3 [4 [5 [6 0]]]]]]) [2 [3 0]])   :: now 2×3
```

Useful operations (all jet-fast):

| call | meaning |
|------|---------|
| `(tshape t)` / `(tdata t)` | the shape list / the flat data list |
| `(tget t [i [j 0]])` | element at a row-major index tuple |
| `(tsum t)` | sum of all elements |
| `(tdot a b)` | dot product of two 1-D tensors |
| `(tadd a b)` `(tsub a b)` `(thad a b)` | elementwise add / subtract / multiply |
| `(tscale t k)` | multiply every element by a signed scalar `k` |
| `(tmatmul a b)` | 2-D matrix multiply `[m k]·[k n] → [m n]` |

```
(tdot (tfromnats [1 [2 [3 0]]]) (tfromnats [4 [5 [6 0]]]))   :: = [0 32000]  (32.0)
```

> **Tip — building lists.** `[a b c 0]` right-nests into the cons list `[a [b [c 0]]]`, so a
> data list is always 0-terminated. The host display prints a shape like `[2 3]` and a signed
> number like `32.000`.

---

## 3. Data visualization (`lib/plot.lat`)

`plot` separates the *interesting* part of charting — scaling data into pixel geometry —
from the *boring* part — emitting SVG text. The Latte side computes geometry; Hymn (the web
server) serializes it to SVG.

- `(maxof xs)` — the largest value (used to scale the axis);
- `(bars xs W H)` — one `[x y w h]` rectangle per value, scaled to fill a `W×H` box;
- `(points xs W H)` — one `[x y]` point per value, for line/scatter charts.

Because SVG's y-axis grows downward, a bar's `y` is `H - height` and its `height` is
`value · H / max`. All coordinates are plain naturals, so layout is pure integer arithmetic.

### Three ways to draw

```sh
# 1. CLI — prints an SVG document (redirect to a file to view)
latte chart bar 3 1 4 1 5 9 2 6  > chart.svg
latte chart line 1 2 3 5 8 13

# 2. The chart GUI — open /chart, type numbers, pick Bar / Line / Scatter

# 3. The HTTP API directly
#    POST /api/plot   body: "line 3 1 4 1 5"   -> an <svg> document
```

### Visualizing a model

The natural use in ML is to plot data against a model's predictions. Compute the predicted
y-values with `ml`'s `predict_all`, take their magnitudes, and chart them next to the data:

```
(predict_all [0 2000] [0 1000] (tdata (tfromnats [1 [2 [3 [4 0]]]])))
:: predictions for w=2, b=1 over x = 1..4  ->  3.0, 5.0, 7.0, 9.0
```

Feed `3 5 7 9` to `latte chart line` (or `/api/plot`) to see the fitted line.

---

## 4. Anatomy of a model (`lib/ml.lat`)

The worked model is **linear regression**: `y = w·x + b`. Three pieces define it.

**1. The prediction.**

```
predict = fn [w b x] -> (nadd (nmul w x) b)
```

**2. The loss** — mean squared error, a single signed number that measures how wrong the
model currently is:

```
mse = fn [w b xs ys] -> ...mean of (predict - y)^2...
```

At the true weights the loss is exactly zero:

```
(mse [0 2000] [0 1000] xs ys)   :: with the exact data -> [0 0]
```

**3. The gradient step** — nudge `w` and `b` downhill. For squared error the gradients are

```
dL/dw = mean( (prediction - y) · x )
dL/db = mean(  prediction - y      )
```

and the update is `w ← w − lr·dL/dw`, `b ← b − lr·dL/db`. `gradstep` does exactly this in
one pass over the data; `train` repeats it for a fixed number of iterations.

---

## 5. Training, and the two limits you will hit

```sh
latte ml --iters 5000        # fits y = 2x+1; learns w = 2.011, b = 0.968
```

or from the console / API:

```
(fit [0 1000 ...build xs...] [...ys...] [0 200] 5000)   :: -> [w b]
(fit_demo 5000)                                         :: the built-in y = 2x+1 example
```

Choosing the **learning rate** `lr` is the whole game, and fixed-point makes the trade-off
unusually concrete:

- **Too large → overflow.** With `lr ≥ 0.25` (`[0 250]`) the updates overshoot, the weights
  grow each step, and a magnitude eventually exceeds the jet's range — training stops with
  `mul jet overflow`. Treat that error as "diverged; lower the learning rate."
- **Too small / near the optimum → underflow.** As the fit improves, the gradients shrink.
  Once `lr · gradient` drops below `0.001`, the update rounds to zero and training **stalls**.
  This is why the demo settles at `w ≈ 2.01, b ≈ 0.97` rather than exactly `2, 1`: it has
  reached the resolution floor of three-decimal fixed point, not a bug.

`lr = 0.2` is the largest stable rate for this problem; it converges in a few thousand
iterations and then sits at the floor.

### Designing your own model

The same three-part recipe generalizes. To fit a different model:

1. **Write `predict`** in terms of your parameters and the signed/tensor ops (use `tdot` for
   a weight vector, `tmatmul` for a layer).
2. **Write the loss** as a function of the parameters and the data (reuse `mse`, or write
   your own with `nsub`/`nmul`).
3. **Derive the gradient** of that loss and write a `gradstep` that subtracts
   `lr · gradient` from each parameter; loop it in a `train` function.

Watch the loss with `mse` as you go — if it is not decreasing, your learning rate is too
high (diverging) or your gradient sign is wrong; if it decreases then freezes well short of
zero, you have hit the fixed-point floor and should accept the approximate fit (or rescale
your data so the meaningful digits sit above `0.001`).

### Practical checklist

- Scale inputs so important differences are larger than `0.001`.
- Start `lr` small (`[0 50]` = 0.05), raise it until training either converges fast or
  overflows, then back off.
- Log `(mse …)` every so often; a stalled-but-low loss means "done, at this precision."
- Plot data vs. `predict_all` with `plot` to see the fit, not just the numbers.

---

## Where things live

| file | role |
|------|------|
| `lib/num.lat` | signed fixed-point numbers |
| `lib/tensor.lat` | n-dimensional tensors |
| `lib/ml.lat` | linear regression: `predict`, `mse`, `gradstep`, `train`, `fit`, `fit_demo` |
| `lib/plot.lat` | chart layout (`maxof`, `bars`, `points`) |
| `latte tensor` / `latte ml` / `latte chart` | CLI demos |
| `/`, `/editor`, `/chart` | the GUI: console, Facet editor, charts (start with `latte`) |

---

## More models

`lib/ml.lat` now carries three further model families beyond linear regression, all written in
Latte over the signed fixed-point library (no transcendental functions required):

| model | arms | demo |
|-------|------|------|
| **Perceptron** — online linear classifier, classes {0,1} | `pscore`, `pclass`, `ppredict`, `pepoch`, `ptrain` | `latte ml perceptron` — learns to classify x>0 vs x<0; predictions `1 1 0 0` |
| **k-means** (k = 2) — unsupervised clustering | `kdist2`, `knearest`, `kmean`, `kstep`, `kmeans2` | `latte ml kmeans` — clusters {1,2,3,10,11,12} to centroids ≈ 2.0 and 11.0 |
| **k-NN** (k = 1) — instance-based classification | `kdist2`, `knn1` | `latte ml knn` — labels a query by its nearest training point |

The perceptron uses the classic mistake-driven update `w ← w + lr·(y − ŷ)·x`; k-means alternates
nearest-centroid assignment with mean recomputation; 1-NN returns the label of the closest point by
squared distance. Each is checked in the test suite (`ml_new_models_learn`).

---

## Chess: a searching evaluator

`lib/chessml.lat` learns piece values by gradient descent (the `trained` weights) and used to play
a 1-ply greedy move. It now adds a **positional evaluation** (`centerbonus`/`evalpos`: a small bonus
for occupying the four central squares) and an **alpha-beta minimax search** (`minimax`, `choose_ab`)
that looks several plies ahead — so the machine no longer hangs pieces or misses recaptures, and it
finds forced mates (verified on a mate-in-one in `chess_ab_search_finds_mate_and_legal_moves`). The
GUI's "play the model" runs `choose_ab` at depth 2 through the compiled engine.

---

## Neural networks (`lib/nn.lat`)

The neural-network library treats a network as **composable data**: an architecture is a *list of
layers*, and a forward pass is a fold (`net_fwd`) over that list. Layers are tagged:

| layer | meaning |
|-------|---------|
| `[%dense [W b]]` | a fully-connected layer, `W·x + b` (W is a list of rows, b a bias vector) |
| `[%relu 0]` | rectified-linear activation, elementwise `max(0, x)` |
| `[%tanh 0]` | a fast fixed-point `tanh` activation |
| `[%res SUB]` | a **residual / skip connection**: `SUB(x) + x` |

Because `%res` carries its own sub-network, you can nest blocks — a `resblock W1 b1 W2 b2` is just
`[%res …]` wrapping two dense layers and ReLUs, exactly the building block of a ResNet. `net_fwd`
runs any such architecture, so deeper nets are built by consing more layers onto the list. The
vector/matrix plumbing (`vadd`, `vsub`, `vmul`, `vscale`, `matvec`, `outer`, `matTvec`, `mscale`,
`msub`) is all in the library, over the signed fixed-point numbers of `num`.

### The newer architectures

The layer set now covers the modern toolkit, all as composable data: `%sigmoid`, `%softmax`
(fixed-point `exp` by four squarings of `1 + x/16`, inputs max-shifted), `%layernorm`,
`%conv1d [K b]` (stride 1, valid padding), and `%res` residual wrappers. **Sequences** (lists of
vectors) get their own layer algebra under `seq_fwd`: `%attn [Wq Wk Wv]` is single-head scaled
dot-product **self-attention**, `%seqdense` is a position-wise dense layer, `%lnorm` normalizes
every position, `%seqres` adds a residual around any sub-stack, and `%pool` mean-pools the
sequence to one vector. A **transformer block** is therefore ordinary data — attention +
residual + layernorm + position-wise dense + residual + layernorm — and `block seed d` builds a
seeded one (`randmat`/`lcg` give reproducible initializations). `latte nn` runs a 3-token
sequence through a seeded block live. Customizability is structural: any composition of these
layers — ResNets, transformer stacks, conv pyramids — is a list you write.

**Training is generic now — reverse-mode autodiff over the layer algebra.** Every vector
layer has a vector-Jacobian product (`layer_bwd`): exact fixed-point derivatives for
relu/tanh/sigmoid (`ntanh x = x/(1+|x|)` differentiates to `1/(1+|x|)²`), the dense VJP
`dx = Wᵀdy` with `dW = dy⊗x`, and rmsnorm with the standard straight-through approximation
(stated, not hidden). `fwd_cache` keeps each layer's input, `stack_bwd` folds the VJPs
backward, and `stack_train` does SGD on ANY composition — the test suite trains both the
classic 2-layer |x| net (loss 10822 → 0) and a deeper tanh/relu stack through the same code
path. Architecture is data; now its gradient is too.

**The post-transformer line is in.** `%ssm` is an S4D/Mamba-style DIAGONAL gated linear
recurrence (`h_t = a⊙h_{t−1} + b⊙x_t`, `y_t = c⊙h_t + d⊙x_t`) — linear-time in sequence
length and, having no softmax or exp, a natural fit for fixed point (which is exactly why
the quantization literature likes SSMs on edge hardware). `%seqlnorm` is per-position
rmsnorm, and `ssmblock seed d` builds the modern PRENORM block: rmsnorm → ssm → residual →
rmsnorm → dense → residual. `latte nn` runs both a transformer block and an SSM block live.
Attention and SSM sequence layers are forward-only in this release; the vector-layer stacks
train end to end.

### Training by backpropagation

Beyond inference, the library trains a one-hidden-layer MLP (ReLU hidden, linear output, MSE loss)
by full backprop: `mlp_fwd`, `mlp_step` (one SGD step over a sample), `mlp_epoch`, `mlp_train`, and
`mlp_loss`. The gradient flows the textbook way — output error `out − y`, then `outer(dout, h)` for
the output weights, `matTvec(W2, dout)` back to the hidden layer, gated by `drelu` on the
pre-activations, then `outer(dz1, x)` for the input weights.

`latte nn` demonstrates it on `y = |x|`, a function no linear model can fit but a two-unit ReLU
hidden layer can. Starting away from the solution the loss falls from ≈ 2.7 to ≈ 0 and the network
reproduces `|−3|,|−1|,|1|,|3| = 3,1,1,3`. The command also writes a **loss curve** (`nn-loss.svg`).
Convergence and the residual-block forward pass are both checked in the test suite
(`nn_mlp_learns_abs_by_backprop`, `nn_composable_residual_forward`).

---

## Visualization for machine learning

`viz.rs` gains a **multi-series line chart** (`render_lines`) — the plot ML actually needs: a shared
y-scale, gridlines with value labels, a titled frame, and a legend. It is what draws the MLP loss
curve and, below, the trading strategy's equity curve against its benchmark. (The earlier single-
series `bar`/`line`/`scatter` charts laid out by `lib/plot.lat` remain for `latte chart` and the
`/chart` GUI page.)

---

## Financial machine learning (`lib/fin.lat`)

`fin.lat` is a small, honest pipeline in the spirit of Lopez de Prado's *Advances in Financial
Machine Learning*. The tool **composes the other libraries** — `num` for arithmetic, `nn` for the
vector/dot-product plumbing, `plot`/`viz` for the chart — into the standard quant workflow:

1. **Features** — `rets` turns a price series into percent returns; `dataset rs k` builds a
   supervised set whose feature vector is the previous `k` returns.
2. **Labels** — fixed-horizon labeling: `1` if the next return is positive, else `0`.
3. **Standardization** — `transpose`/`colstats`/`zrow` z-score the features using **training-set
   statistics only**, so no information leaks from the future.
4. **Walk-forward split** — the first part of the series trains, the later part tests; the data is
   never shuffled, which is the single most important guard against look-ahead leakage.
5. **Model** — multi-feature **logistic regression** (`lr_score`/`lr_prob`/`lr_pred`/`lr_step`/
   `lr_train`), fit by batch gradient descent with the fast fixed-point `nsigmoid`.
6. **Evaluation** — `lr_acc` and a majority-class `base_acc`, plus a strategy/buy-&-hold equity
   curve (`gold_run`).

### Demo: predicting gold/USD

`latte fin gold` runs the whole pipeline on **480 real daily XAU/USD closes (2020–2022)** embedded
in the binary (sourced from a public daily-bars dataset). With five lagged returns as features and a
70/30 walk-forward split it reports:

```
training accuracy : 49.2%
TEST accuracy     : 50.3%   (out-of-sample)
baseline (majority): 50.3%
edge over baseline: +0.0 pts
```

That is the *correct* and honest result, not a disappointing one: daily price direction is close to
a coin flip, because financial data is low signal-to-noise — exactly de Prado's thesis. The same
classifier earns a **+50-point** edge on a synthetic mean-reverting series
(`fin_logistic_learns_mean_reversion` in the test suite), so the model works; the gold market simply
has no easy linear signal at this horizon. `latte fin gold` also writes `gold-fin.svg`, the
out-of-sample equity of the strategy against buy-and-hold. The deliverable is a working, leak-aware
pipeline — not a profit machine, and the tool says so.

---

## A graphics library (`lib/gfx.lat`)

`gfx` makes drawings *data*. A scene is a 0-terminated list of tagged shapes — `[%line …]`,
`[%rect …]`, `[%circle …]`, `[%poly …]`, `[%text …]` — built with Latte constructors over
packed-RGB colours (`rgb r g b`). The host renderer (`src/gfx.rs`) walks the noun and serializes
it to SVG; the *structure* of the picture lives entirely in Latte, exactly the split the chart
library uses for plots. `latte gfx` builds a demo scene (a little house) and writes it to SVG, and
the `/gpu` GUI page renders it live via `POST /api/gfx`. Two unit tests cover the renderer.

## A GPU compute library (`lib/gpu.lat`)

`gpu` is a *data-parallel programming model* for Latte. Programs describe GPU work as data: a
device, buffers (lists of numbers), and a pipeline of kernels — `map`, `zipWith`, `reduce`,
`saxpy`, `dot`, `matmul`, and a per-pixel `shade`. The **target device is an NVIDIA GeForce RTX
4070 Ti SUPER** (16 GB VRAM — the card's real configuration — 8448 CUDA cores, 66 SMs). The kernel
set, buffer model, and call sites are exactly what a CUDA backend would use, so dispatching to the
card is a drop-in backend swap.

In this build there is no CUDA driver and there are zero external crates, so the *active* backend
is a genuine multi-core CPU backend (`std::thread::scope`) that runs each kernel in parallel across
the host's cores — honest about what is executing while keeping the device-facing API intact.

It integrates with the other libraries:
- **ML.** `matmul` is the core neural-network kernel (the dense layer of `nn.lat`). `latte gpu`
  benchmarks the parallel matmul against a serial reference and checks they agree to the bit.
- **GFX.** The per-pixel Mandelbrot `shade` kernel — the canonical embarrassingly-parallel GPU
  workload — produces a colour field that is rendered through the graphics path to an SVG raster.

`latte gpu` prints the device, runs the matmul benchmark, executes a small Latte `gpu` program
(`saxpy` then `reduce`) on the backend, and writes the Mandelbrot image. The `/gpu` GUI page shows
all of it live via `POST /api/gpu`. Three unit tests cover matmul (parallel == serial), the
parallel primitives, and the shader.

---

## Financial ML, continued: choosing the right market

The gold experiment (above) was the honest negative result that pointed the way: a price-only model
is near-chance on gold out-of-sample. So **which market should a price-only momentum/volatility
model be applied to?** The literature is consistent — time-series momentum and volatility clustering
are *strongest in cryptocurrency* (Moskowitz, Ooi & Pedersen 2012 on time-series momentum across
asset classes; Shen, Urquhart & Wang 2022 on Bitcoin momentum; Katsiampa 2017 on Bitcoin GARCH
volatility). Crypto trades 24/7, is highly volatile, and shows persistent ARCH effects.

So the demo now targets **Bitcoin (BTC/USD)** — 1300 daily closes spanning a 2022 bear bottom
through the 2023–2025 recovery (Coin Metrics community data). The features are unchanged in spirit
(multi-horizon momentum + realized volatility); the default task is **next-day volatility-regime
prediction** (is tomorrow a high- or low-|return| day, relative to the training-set median). On the
walk-forward out-of-sample test this reaches roughly **55% vs a ~51% baseline — a genuine +3–4 point
edge**, stable across training lengths. Daily *direction* remains near-chance even here, which is the
honest finding: the predictable thing in markets is *volatility*, not direction, and crypto is where
that predictability is strongest. `latte fin` runs it (`--dir` for the direction variant, `--vol` for
volatility, `--horizon N` for the direction horizon); the `/fin` GUI page runs it live.

---

## GPU auto-detection (use it when present, never rely on it)

The compute backend is chosen automatically. `gpu::detect_backend()` probes the host with pure
filesystem/PATH checks (no external crates, never panics): the NVIDIA kernel driver's
`/proc/driver/nvidia/gpus/*/information`, the `/dev/nvidia0` device node, and `nvidia-smi` on
`PATH`. If a device is found it returns `Backend::Cuda(name)` (a real CUDA build would dispatch
kernels to it); otherwise `Backend::Cpu(lanes)`. The ML kernel path (`matmul_auto`) and the GUI's
compute page both go through the detected backend, so a present GPU accelerates them and an absent
one transparently falls back to the multi-core CPU backend. `latte gpu` prints the detection result
("GPU detected: yes/no") and the active backend; on a machine without a GPU it correctly reports
"no" and runs on CPU. A test asserts detection never panics and that the auto-dispatched matmul
always equals the serial reference regardless of backend.

## Sentiment analysis from news (`lib/sentiment.lat` + `src/sentiment.rs`)

Lexicon-based financial sentiment after Loughran & McDonald (2011), who showed general-purpose
dictionaries misclassify finance text and built finance-specific word lists. The scorer tokenizes
text in the host, tallies positive/negative finance words from an embedded subset of the LM lists,
and computes polarity = (pos − neg) / (pos + neg) — with the ratio arithmetic done in Latte
(`polarity`, signed fixed-point), so the language is in the loop. `latte sentiment "<text>"` (and
`POST /api/sentiment`) returns the counts, polarity, and a BULLISH/neutral/BEARISH label; validated
on real headlines (a "rally/recover/surge higher" headline scores +1.0, a "selloff/falter/recession
fears" headline −1.0). News sentiment is most useful on information-driven markets (equities,
indices, single names), where it is an exogenous feature fed to the model and the trading advisor.

## Real, recent, live market data (`latte fetch`)

The models run on **real data**. The embedded series is daily BTC/USD closes — Coin Metrics
community data exact through 2026-05-23, then dated press reports (Fortune, Yahoo Finance, CNBC,
The Block, Investing.com) through 2026-06-10 — and `latte fetch` refreshes it at run time from
the live Coin Metrics file (HTTPS via `curl`, the same external-tool trust model Anvil uses for
`rustc`), caching under `~/.cache/orpheus/market/`. Every consumer takes a `--live` flag:
`latte ta --live`, `latte trade --live`, `latte chart market --live`. Each report states its
provenance ("live Coin Metrics fetch (5789 days) + 18 embedded recent days to 2026-06-10"), and
when the community file lags, the embedded press-anchored tail is spliced on so the freshest
real closes are never lost. No network, no curl? The embedded series serves, and everything
still works.

A bundled corpus of **real, dated headlines** about the market (`MARKET_NEWS`, gathered
2026-06-10 with sources) feeds the sentiment side; `--news FILE` substitutes today's headlines
(one per line, optionally `YYYY-MM-DD<TAB>Source<TAB>headline`).

## Technical analysis in Latte (`lib/ta.lat`, `latte ta`)

Five classical indicators, written entirely in Latte over signed fixed-point and computed on
Loom: **SMA/EMA**, **rate-of-change** momentum, Wilder's **RSI(14)**, **MACD(12,26,9)** with its
signal and histogram, and **Bollinger %B(20,2σ)** — plus realized volatility. `votes` has each
indicator cast a −1/0/+1 verdict (trend vs SMA50, 10-day ROC, RSI overbought/oversold, MACD
histogram sign, %B band touch) and `score` sums them into a composite in −5…+5. Run it standalone:

```
latte ta            # the verdict table on the freshest cached/embedded data
latte ta --live     # fetch first
```

or in the System GUI (`ta` embeds the table in the Log) and over HTTP (`POST /api/ta`).

## The trading advisor (`latte trade`) — technical analysis × news sentiment

The advisor **fuses two kinds of evidence** and sizes the position with the volatility model:

1. **Technical analysis (60%)** — `ta.lat`'s composite vote, normalized to a signal in [−1, +1].
2. **News sentiment (40%)** — every bundled (or `--news`-supplied) headline is scored by the
   FUSED scorer: 0.65 × the trained classifier (see "The trained text classifier" below) +
   0.35 × the Loughran-McDonald lexicon (whose polarity arithmetic runs in
   `lib/sentiment.lat` on Loom), aggregated with a 3-day-half-life recency weight;
   `--sentiment S` overrides.
3. **The combined lean** `0.6·TA + 0.4·news` decides direction: above +0.1 → long candidate;
   below −0.1 → bearish → **FLAT** (no shorting in this demo); in between → no edge, FLAT.
4. **Sizing** — **fractional Kelly** from the *measured* out-of-sample momentum hit rate (the
   edge is halved when momentum disagrees with the combined lean), nudged by sentiment, times
   **volatility targeting** against the volatility model's regime call (the dependable edge),
   capped at 2× leverage. The volatility model trains once per process and is cached.

The report shows its evidence: the five-indicator table, every scored headline with its date and
polarity, the fusion arithmetic, the measured reliabilities, and the sizing chain. Flags:
`--account N --kelly F --live --news FILE --sentiment S`. The `/trade` GUI page renders the same
evidence; `trade` in the System GUI embeds it in the Log. Every output carries an explicit
disclaimer: this is a research demonstration, not financial advice.

## The trained text classifier — headlines, reports, and documents

The research verdict on financial sentiment is consistent: learned models beat word-counting
lexicons, because lexicons cannot read context or negation. Orpheus's answer, inside the
zero-dependency rule, is a **logistic regression over unigram + bigram features** (stopwords
filtered, negators kept), trained offline on a labeled corpus of financial text in BOTH
registers — market headlines *and* report/filing language — with the failure cases lexicons
get wrong. The ~200 significant weights are embedded in `src/sentiment.rs`; training is
reproducible from the corpus. The canonical contrasts, straight from the test suite:

- "ETF **outflows eased** as the market stabilized" → lexicon −1.0 (it counts "outflows");
  model **+0.99** (it learned the bigram).
- "the company **failed to deliver**" → model strongly negative; "risks **did not
  materialize**" positive vs "risks **materialized**" negative — negation read correctly.
- Out-of-vocabulary text scores ≈ 0 (the intercept is regularized to neutral).

**It scores documents, not just headlines.** The scorer is sentence-level: `score_document`
splits a report, scores each sentence, and aggregates with confidence weights (decisive
sentences count more; boilerplate self-mutes), returning the per-sentence evidence. So the
same machinery reads a quarterly review, an analyst note, or a filing:

```
latte sentiment "ETF outflows eased as the market stabilized"   # all three scores shown
latte sentiment --file q2-review.txt                            # document mode
latte sentiment --doc visualization-and-ml                      # score a docs/ page
```

`POST /api/sentiment` returns the per-sentence JSON; in the System GUI, `System.Sentiment
<text>` scores in place and **`Doc.Score <name>`** opens a whole document's evidence table in
its own viewer. The advisor's news leg uses the fused score everywhere.

## Honest validation: embargo, costs, HAR-RV, conformal bands

Four methodology upgrades keep the advisor's claims defensible:

- **Embargoed walk-forward** — the out-of-sample momentum hit rate leaves a 10-day gap
  between the training span and the test slice, so overlapping windows cannot leak
  (the purged/embargoed-CV discipline, López de Prado 2018, in single-split form).
- **Transaction costs** — the strategy equity curves in `lib/fin.lat` charge 5 bp on every
  position change before compounding; a signal that trades often must beat its own turnover,
  and the chart says so in its title.
- **HAR-RV** (Corsi 2009) — next-day volatility is forecast by the daily/weekly/monthly
  realized-volatility regression (least squares on the full history, R² reported), and that
  forecast — the dependable edge in this data — drives the volatility-targeted sizing.
- **A conformal band** — the report states a distribution-free 90% band on tomorrow's move
  (the split-conformal quantile of the trailing year's |returns|), so the uncertainty is a
  measured quantity, not an adjective.

## Charts in your reports — embeddable visualization

Reports written in the GUI (documents in `/docs`, pages in `/editor`) can **embed live charts**
as fenced blocks; the renderer posts the block to the visualization engine and inlines the SVG:

    ```chart
    Equity curves              <- title line
    strategy | buy & hold      <- series labels
    1 2 3 4 5                  <- one series per line
    2 2 3 3 4
    ```

    ```chart market
    days=120                   <- the real price series + SMA20/SMA50
    live=1                     <- optional: refresh from Coin Metrics first
    ```

    ```chart bar
    3 1 4 1 5 9 2 6            <- also: line, scatter (layout in lib/plot.lat)
    ```

The same charts are one command away everywhere: `latte chart market --live --days 90 > m.svg`
on the CLI, `chart market days=120` embedded in the System GUI's Log, `POST /api/marketchart`
and `POST /api/plotn` over HTTP. The "＋ New" button in `/docs` starts a fresh document from a
template that demonstrates the blocks — so producing new documentation with live figures, from
inside the GUI, is the default path, not a trick.

## A fuller standard library

Independent of the tools above, `std.lat` gained general-purpose list machinery so the language is
fuller on its own terms: predicate combinators `any` / `all` / `count`, `maximum` / `minimum` /
`argmax`, `enumerate`, `scanl` (prefix folds), and `concat`. These are unit-tested (e.g. `argmax`
of `[3 1 4 1 5 9 2]` is 5) even where no current tool calls them.
