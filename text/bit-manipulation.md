# Bit manipulation — an interactive tour

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/bits.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

Bit manipulation turns the binary representation of a number into a tool: whole sets fit
in one integer, membership is a shift-and-mask, and a few XOR identities collapse problems
that look like they need a hash map down to a single pass with no extra memory.

This tour rests on a small **language addition**: Latte's atoms are unsigned
arbitrary-precision integers, but the language had no bitwise operators, so they were added
to the standard library — `band`, `bor`, `bxor`, `shl`, `shr`, `bit`, `popcount`, `lowbit`.
The shifts are exact (`shl n k` is `n * 2^k`); AND/OR/XOR fold the numbers bit by bit.
Everything below builds on those.

The one set of identities to keep in mind: `x ^ x = 0`, `x ^ 0 = x`, and XOR is
commutative and associative — so XOR-ing a value in twice cancels it completely.

## 1. XOR tricks — self-cancellation does the bookkeeping

**Cue:** "every element appears twice except one", "find the missing/duplicate", "without
extra space". **Idea:** XOR-ing a value an even number of times leaves nothing behind.

**The algebra that makes this airtight.** Under XOR the integers form an *abelian group*: it
is associative and commutative (so the order you fold in values is irrelevant), `0` is the
identity (`x ^ 0 = x`), and every element is *its own inverse* (`x ^ x = 0`). Those three
facts are the whole trick. Because order does not matter, you can mentally reorder any list so
the duplicates sit next to each other; because each duplicated pair XORs to `0` and `0` is the
identity, every pair vanishes; what remains is the one unpaired value. The same reasoning
powers the missing-number trick — XOR the present values *and* all the indices, and everything
that appears in both lists cancels, leaving only the index that has no value. No counting, no
hash set, `O(1)` space.

**Single number** — in a list where everything is paired except one loner, XOR the whole
list and the pairs annihilate, leaving the answer:

```tool eval (bm_single (bm_dup0 0))
```

**Missing number** — the list is `0..n` with one value gone. XOR all the list values
*and* all the indices `0..n`; every present number cancels, the gap survives:

```tool eval (bm_missing (bm_gap0 0) 3)
```

The most striking one: **add two numbers without `+`**. XOR is addition with every carry
ignored; `(a AND b) << 1` is exactly those carries; repeat until there is no carry left.
This is how a hardware adder works, expressed in software:

```tool eval (bm_add 5 3)
```

It is not a toy that only works on small inputs — `123 + 456`:

```tool eval (bm_add 123 456)
```

## 2. Bit tests — reading and writing one bit

**Cue:** flags packed into an integer, "is the i-th option on", "is it a power of two".

**Power of two**: a positive number is a power of two iff it has a single set bit — and
that holds iff `n AND (n-1)` is zero, because subtracting 1 flips the lowest set bit and
everything below it. `16` qualifies (`1` = yes):

```tool eval (bm_ispow2 16)
```

…but `12` (`1100`) has two set bits (`0` = no):

```tool eval (bm_ispow2 12)
```

Setting a bit ORs in `1 << i`, so setting bit 3 of 0 gives 8:

```tool eval (bm_set 0 3)
```

…and clearing the **lowest** set bit, `n AND (n-1)`, is the move that powers "iterate the
set bits" loops — `12` (`1100`) loses its low one to become `8` (`1000`):

```tool eval (bm_clearlow 12)
```

## 3. Subsets as bitmasks

**Cue:** "try all subsets", "bitmask DP", "at most ~20 items". **Idea:** an `n`-element set
has exactly `2^n` subsets, and each integer in `[0, 2^n)` *is* a subset — bit `j` says
whether element `j` is in. Iterating the integers enumerates every subset:

```tool eval (bm_subsets (bm_set0 0))
```

That is eight subsets for three elements (`2^3`), from the empty set up to the whole set:

```tool eval (len (bm_subsets (bm_set0 0)))
```

This integer-as-subset encoding is the entire trick behind bitmask dynamic programming
(travelling-salesman over subsets, assignment problems, and so on).

**Why this is exactly right, and why "~20" is the ceiling.** The encoding is a *bijection*:
every subset corresponds to one and only one integer, because each element independently is
either in (bit 1) or out (bit 0), and `n` independent binary choices produce exactly `2^n`
distinct patterns — no subset is missed and none is counted twice. That same `2^n` is also the
catch: the count is *exponential*, so enumerating all subsets is only feasible for small `n`
(around 20, where `2^20 ≈ 10^6` is still cheap). Bitmask DP pushes that a little further by
storing one value *per subset* and reusing overlapping work, but the exponential `2^n` table
is why these techniques live in the "n ≤ 20" corner of the interview space.

## 4. Counting bits

**Cue:** "how many bits differ", "population count", "parity".

**Hamming distance** — how many positions two numbers differ in — is just the popcount of
their XOR. `4` is `100`, `1` is `001`, differing in two positions:

```tool eval (bm_hamming 4 1)
```

And **counting the set bits of every number** `0..n` is a tidy little DP:
`popcount(i) = popcount(i >> 1) + (i AND 1)`, since shifting right drops the lowest bit.
For `0..7`:

```tool eval (bm_countbits 7)
```

---

Bit manipulation rounds out the warm-up and trick tier of the interview toolkit, alongside
the paradigms, the data-structure patterns, the weighted-graph algorithms, and the
number-theory set. Every arm here is in `lib/bits.lat` (prefix `bm_`), built on the bitwise
operators now in `std`, and `latte bits <topic>` prints these in a terminal (topics: xor,
tests, enumeration, counting).
