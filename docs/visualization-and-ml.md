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
