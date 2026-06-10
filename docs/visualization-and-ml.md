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
