# Start here — how to read these tutorials

This page is the on-ramp. The technique tutorials explain clever algorithms, but they assume
you can already read the little code snippets and know a few conventions. This primer teaches
exactly those things — in about ten minutes — so that afterwards *any* tutorial here is
approachable. You do not need prior experience with this system; you only need to be willing to
read slowly and try the live examples.

## 1. These pages actually run

Every framed command in a tutorial is **live**. When the page loads, the system runs the real
code and drops the real result underneath. You are never reading a screenshot — you are reading
an experiment that just happened. Here is one:

```tool eval (add 2 3)
```

That panel shows `5`, computed just now. To run a line yourself, **middle-click** it (or put the
text cursor on it and press Ctrl/Cmd+Enter). Try changing the `2` to a `4` in the line above and
re-running it. Learning to *predict the result, then check it* is the single most effective way
to study these tutorials.

## 2. Reading the code in five minutes

The snippets are written in **Latte**, a small functional language. You only need a handful of
shapes to follow along (the full reference is the `latte-language` manual).

- **Calls put the function first.** `(add 2 3)` means "add 2 and 3". `(max 7 4)` is 7. This is
  the same as `add(2, 3)` in other languages, just with the parenthesis moved.

```tool eval (max 7 4)
```

- **Numbers are whole and never negative.** There are no decimals and no negative numbers; the
  values are the counting numbers 0, 1, 2, …. A short text is written with a `%`, so `%a` is the
  single character "a".
- **Lists end in `0`.** A list is written inside brackets with a trailing `0` that marks "the
  end": `[1 2 3 0]` is the list one, two, three. The `0` is just the full-stop. A *two*-item
  bracket like `[5 9]` is a **pair** (it does not end in `0`) — tutorials use pairs to bundle two
  things, e.g. an interval `[start end]`.
- **`fn` makes a function, `let` names a value.** `fn [n] -> (mul n 2)` is "given n, double it".
  `let x = 5 in (add x 1)` is 6.
- **`loop … again(…) end` repeats.** A loop carries a few running values; `again(…)` starts the
  next round with new values; the loop ends when a condition is met. You will see this shape a
  lot — read it as "keep going, updating these values, until done".
- **Each library groups its tools under a short prefix.** In the binary-search tutorial every
  tool starts with `se_`; in dynamic programming, `dp_`. So `se_lowerbound` is the "lower bound"
  tool from the search library. Many tutorials also ship ready-made sample inputs ending in `0`,
  like `(se_sorted0 0)` — a sorted list to experiment on.

That is genuinely enough to read every snippet. When something deeper comes up, the comment next
to the code in the library explains it line by line.

## 3. The conventions every tutorial assumes

These four conventions trip up newcomers more than the algorithms do. Learn them once here.

**Yes is `0`.** This is the surprising one. Questions like "is this a palindrome?" answer with a
*loobean*, where **`0` means yes/true** and `1` means no/false. It feels backwards at first, but
it is consistent everywhere. So when a tutorial says a check "returns 0", that means **yes**.
Comparisons work the same way: a "less-than" test returns `0` when the first really is smaller.

```tool eval (se_contains (se_sorted0 0) 3)
```

That panel asks "does the sorted sample contain 3?" and answers `0` — meaning **yes, it does**.

**An interval is `[start end]`.** Wherever a tutorial talks about ranges or meetings, one
interval is the pair `[start end]` — `[2 6]` is the span from 2 to 6.

**A tree node is `[value [left right]]`, and empty is `0`.** A leaf is `[value [0 0]]`; a missing
child is `0`. So a node bundles its value with the pair of its two children.

**You cannot subtract below zero.** Because numbers are never negative, subtracting a larger
number from a smaller one is an error, not a negative result. That is why the code so often
*checks* before subtracting (e.g. "only compute `mid − 1` when `mid` is at least 1"). When you
see a guard like that, this is why.

## 4. Big-O in one minute

Tutorials describe how *fast* an algorithm is with "Big-O" notation. Read it as "how the work
grows as the input grows":

- **O(1)** — constant: the same tiny amount of work no matter how big the input.
- **O(log n)** — logarithmic: the work grows very slowly; doubling the input adds just one step.
  Binary search is the classic example.
- **O(n)** — linear: do a fixed amount of work per item. A single pass over a list.
- **O(n log n)** — a good sorting speed.
- **O(n²)** — quadratic: work for every *pair* of items; fine for small inputs, slow for large.

Smaller is better. Much of the cleverness in these tutorials is about turning an obvious O(n²)
idea into an O(n) or O(log n) one.

## 5. A worked example — study it the slow way

Let us read one algorithm the way you should read all of them. The **lower bound** of a value in
a sorted list is the position of its first occurrence. Take the sorted sample `[1 2 2 2 3 4]` and
look for `2`.

The idea: keep a range `[lo, hi)` that must contain the answer, and halve it each step. Start
with the whole list, `lo = 0` and `hi = 6`. Look at the middle; if the middle value is *less
than* 2, the answer must be to the right, so move `lo` past the middle; otherwise the middle
might be the answer, so pull `hi` down to it. Trace it by hand:

- `lo = 0, hi = 6`, middle is index 3 (value 2). Not less than 2, so `hi = 3`.
- `lo = 0, hi = 3`, middle is index 1 (value 2). Not less than 2, so `hi = 1`.
- `lo = 0, hi = 1`, middle is index 0 (value 1). Less than 2, so `lo = 1`.
- `lo = 1, hi = 1`: the range is empty, so the answer is `lo = 1`.

The first `2` sits at index 1. Now check the hand-trace against the live machine:

```tool eval (se_lowerbound (se_sorted0 0) 2)
```

It says `1`. Notice the rhythm: each step threw away half of what was left, which is why this is
O(log n) — for a million items it would finish in about twenty steps. *That* is the payoff the
search tutorial is teaching, and you just watched it happen.

## 6. How to study any tutorial here

A reliable method, in five small steps:

1. **Read the plain problem** in the opening paragraph — what goes in, what comes out — before
   anything else.
2. **Trace a tiny input by hand**, like we just did, until you can predict the answer.
3. **Read the live result** and confirm it matches your prediction. If it does not, find out why
   before moving on; that gap is exactly where the learning is.
4. **Read the "why it works"** paragraph — the invariant or argument — now that you have a
   concrete example in your head to attach it to.
5. **Change a live cell** and predict the new result before re-running. Owning one example beats
   skimming ten.

Work one section at a time; do not rush to the end. Every tutorial is self-contained, so you can
start with whichever problem sounds most interesting.

## 7. A tiny glossary

- **arm** — one named function inside a library (e.g. `se_lowerbound`).
- **cord** — a short piece of text, written `%abc`.
- **loobean** — a yes/no value where `0` is yes/true and `1` is no/false.
- **pair** — a two-item bundle `[a b]` (no trailing `0`), like an interval or a tree node's
  children.
- **invariant** — something that stays true at every step of a loop; the reason an algorithm is
  correct.
- **O(...)** — Big-O, how the running time grows with the input size.

---

That is the whole on-ramp. Open the organized list of tutorials with
[the tutorials index ▸](run: System.OpenText tutorials), or jump to a gentle first one such as
[Data structures & patterns ▸](run: System.OpenText dsa-techniques). The deeper language and
library references are in the manuals (`latte-language`, `interview-techniques`).
