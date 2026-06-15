# Number theory & math — an interactive tour

**New here?** Two conventions used throughout these tutorials: answers follow the *loobean*
rule where **`0` means yes/true** (so a `0` answer to “is this sorted?” means *yes*), and a
list is written `[a b c 0]` with a trailing `0` marking the end. The primer **Start here**
(`System.OpenText start-here`) teaches how to read the code and run these live examples from
scratch in about ten minutes — then any tutorial here is approachable.

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/numth.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

These are the math warm-ups an interview opens with, and the building blocks under
hashing, cryptography, and combinatorics. Unlike the other tours, nothing here needs a
data structure — only integer arithmetic — so it is the most self-contained set.

A reminder the code comments keep making: in Latte the comparison gates are *loobeans*
where **0 means true** (`gt a b` is 0 when a>b), and `sub` errors on underflow, so every
subtraction is on values known to be non-negative.

## 1. GCD and LCM — Euclid's algorithm

**Cue:** "reduce a fraction", "common period", "tile a rectangle exactly". **Idea:** the
greatest common divisor satisfies `gcd(a, b) = gcd(b, a mod b)`, and `gcd(a, 0) = a`. Each
step replaces the pair with a strictly smaller remainder, so it lands in `O(log n)` steps —
one of the oldest algorithms still in daily use.

```tool eval (nt_gcd 48 36)
```

Two numbers with no common factor are *coprime*, and their GCD is 1:

```tool eval (nt_gcd 17 5)
```

The least common multiple falls right out: `lcm(a, b) = a / gcd(a, b) * b` (dividing first
keeps the product small and exact):

```tool eval (nt_lcm 4 6)
```

## 2. Primes — trial division and the Sieve

**Cue:** "is it prime", "list the primes", anything factor-related. The direct test divides
`n` by every candidate up to `sqrt n` (we loop while `i*i <= n` to avoid a square root). It
returns `0` for prime (the system's "true"):

```tool eval (nt_isprime 7)
```

…and `1` for composite — `9 = 3 × 3`:

```tool eval (nt_isprime 9)
```

To get *all* primes up to `n`, the **Sieve of Eratosthenes** beats testing each number. Start
assuming everything is prime; for each surviving number, strike out its multiples. Whatever is
never struck is prime. Here are the primes up to 30:

```tool eval (nt_sieve 30)
```

## 3. Modular exponentiation — big powers, small numbers

**Cue:** "compute a^b mod m", hashing, and every public-key crypto question. Computing `a^b`
and *then* taking the remainder would overflow instantly; instead we reduce mod `m` at every
step and **square** our way up the exponent, so `b` is consumed one bit at a time in `O(log b)`
multiplications. This is the single operation that makes RSA and Diffie–Hellman feasible.

`2^10 mod 1000` is `1024 mod 1000 = 24`:

```tool eval (nt_modpow 2 10 1000)
```

The point is that the *exponent* can be enormous and the work stays tiny — `7^256 mod 13`
never builds a 200-digit number:

```tool eval (nt_modpow 7 256 13)
```

**Why two independent ideas combine here.** The *square-and-multiply* trick reads the exponent
in binary: squaring the base repeatedly gives `a^1, a^2, a^4, a^8, …` and you multiply the
result by exactly those powers whose bit is set, so a `b`-bit exponent costs `O(log b)`
multiplications instead of `b`. Separately, *reducing mod m after every multiply* keeps every
intermediate below `m²` — this works because modular arithmetic is a *homomorphism*:
`(x·y) mod m = ((x mod m)·(y mod m)) mod m`, so reducing early never changes the final answer.
Neither idea alone suffices: square-and-multiply without reduction still produces astronomically
large integers; reduction without square-and-multiply still loops `b` times. Together they turn
`a^b mod m` — the bottleneck of RSA on thousand-bit numbers — into a few thousand small
multiplications.

## 4. Combinatorics — counting without overcounting

**Cue:** "how many ways", "number of paths", "choose k of n". The factorial counts orderings:

```tool eval (nt_fact 5)
```

The **binomial coefficient** `C(n, k)` counts the ways to choose `k` items from `n`. We build
it multiplicatively — `C(10, 3)` — so we never form the giant factorials `10!` and `7!` only to
cancel them:

```tool eval (nt_choose 10 3)
```

**Why the multiplicative form stays an exact integer.** We compute `C(n,k)` as the running
product `((n−k+1)/1)·((n−k+2)/2)·…·((n)/k)`, dividing as we go. It is not obvious those
divisions come out whole — but they must, because after multiplying in `i` consecutive
integers the partial product is divisible by `i!` (any block of `i` consecutive integers
contains a multiple of every factor of `i!`). So each intermediate is itself a binomial
coefficient, always integral, and the numbers stay small instead of exploding into `10!`
and shrinking back. We also exploit the symmetry `C(n,k) = C(n,n−k)` to loop the smaller
of `k` and `n−k` times — choosing 3 of 10 is the same as excluding 7.

And **Fibonacci**, computed iteratively (carrying the running pair) rather than with the
exponential naive recursion — `F(10) = 55`:

```tool eval (nt_fib 10)
```

## 5. Digit operations — peeling base-10

**Cue:** "sum/reverse the digits", "happy number", "palindrome number". The trick is `mod 10`
to read the last digit and `div 10` to drop it, repeated until nothing is left. Digit sum of
1234:

```tool eval (nt_digitsum 1234)
```

…and reversing the digits (the same loop, building the answer up by `*10`):

```tool eval (nt_reverse 1234)
```

---

That rounds out the math warm-ups. Together with the paradigms (*algorithm-techniques*), the
data structures and patterns (*data structures & coding patterns*), and the weighted graphs
(*weighted graphs*), the interview toolkit is well covered. Every arm here is in
`lib/numth.lat` (prefix `nt_`), and `latte numth <topic>` prints these in a terminal (topics:
divisibility, primes, modular, combinatorics, digits).
