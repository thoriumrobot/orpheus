# String algorithms — an interactive tour

This is a working tutorial, not a screenshot. Every framed panel is a **live object**:
when the text loads, the system evaluates a real Latte expression against `lib/strings.lat`
and embeds the result. Edit any command line and middle-click it to re-run.

String questions are among the most common in interviews, and they all start from the same
move: turn the string into something you can iterate. In Latte a string is a *cord* — an
atom whose bytes are the characters — so this module rests on the cord↔byte-list helpers
added to `std` (`bytes` turns `%abc` into `[97 98 99]`, `frombytes` turns it back) plus the
general `sort`. A single character is just a one-byte cord: `%a` *is* the number 97, so a
character and a byte compare with `==`.

Results follow the system's loobean convention where **0 means true** (here: 0 = "yes /
found / matches").

## 1. Analysis — reverse, palindrome, count

**Cue:** the warm-up tier. **Reversing** a string is reversing its bytes and rebuilding the
cord:

```tool eval (st_rev %hello)
```

A **palindrome** is a string equal to its own reverse — comparing the two cords as atoms is
exact. `%racecar` qualifies (`0` = yes):

```tool eval (st_ispalin (st_word0 0))
```

…and `%hello` does not (`1` = no):

```tool eval (st_ispalin (st_word1 0))
```

**Counting** a character walks the bytes and tallies matches — the letter `l` appears twice
in `%hello`:

```tool eval (st_count %hello %l)
```

## 2. Anagrams — sorted-character signatures

**Cue:** "are these permutations of each other", "group the anagrams". **Idea:** two strings
are anagrams exactly when their *sorted* characters match, so sorting gives a canonical
signature. `%listen` and `%silent` share one (`0` = anagrams):

```tool eval (st_anagram (st_anaA 0) (st_anaB 0))
```

A close cousin is the **first non-repeating character**: tally every character, then return
the first one in reading order whose count is 1. In `%hello`, `h` is the first that never
repeats:

```tool eval (st_firstuniq %hello)
```

## 3. Substring search — naive, then Rabin-Karp

**Cue:** "find the pattern", "does the text contain". The **naive** method slides a window
the width of the pattern across the text and compares. It finds `%cad` starting at index 4
of `%abracadabra`:

```tool eval (st_find (st_text0 0) %cad)
```

Re-comparing every window costs `O(n·m)`. **Rabin-Karp** fixes that with a *rolling hash*:
hash the pattern once, then slide a same-width hash across the text, subtracting the byte
that leaves and folding in the byte that enters — each step is `O(1)`. On a hash match it
still verifies the bytes (hashes can collide). It finds the first `%bra` at index 1:

```tool eval (st_rabinkarp (st_text0 0) %bra)
```

**The mechanics, and why the verify step is not optional.** The window is hashed as a
polynomial in a base `B` (256 here): `h = b₀·B^(m-1) + b₁·B^(m-2) + … + b_{m-1}`, all mod a
large prime `M`. Sliding right one position is then pure algebra — subtract the leaving byte's
top-place contribution `b₀·B^(m-1)`, multiply the rest by `B` to shift every byte up a place,
and add the entering byte in the now-empty ones place — which is why each slide is `O(1)`
rather than a fresh `O(m)` hash. But a hash is a *fingerprint*, not the string: two different
windows can share a hash (a collision), so a hash match only makes the window a *candidate*
that must be confirmed byte-for-byte. With a good `M` collisions are rare, so verification
almost never fires and the average cost stays `O(n + m)` — though an adversarial text could
force `O(n·m)`, which is why cryptographic contexts randomise `B`.

When the pattern is absent both methods return the text's length (an impossible index, used
as the "not found" sentinel) — here `%xyz` is not in an 11-character string:

```tool eval (st_rabinkarp (st_text0 0) %xyz)
```

The rolling-hash idea — and the polynomial hash it uses — is the same machinery behind the
`dhash` library's Bloom-filter and Merkle hashes, so this connects the string tier back to
the data-intensive systems.

## 4. Families of strings

**Cue:** autocomplete, "common prefix", compression. The **longest common prefix** of a list
of strings is found by folding a pairwise prefix across them (it can only shrink). For
`%flower`, `%flow`, `%flight` it is `%fl`:

```tool eval (st_lcp (st_list0 0))
```

And **run-length encoding** collapses each maximal run of one character into a `[char count]`
pair — the simplest compression scheme and a frequent warm-up. `%aaabbc` becomes three runs:

```tool eval (st_rle (st_run0 0))
```

---

Strings round out the toolkit's coverage of the recurring interview areas: paradigms
(*algorithm-techniques*), data-structure patterns, weighted graphs, number theory, bit
manipulation, and now string algorithms. Every arm here is in `lib/strings.lat` (prefix
`st_`), built on the cord helpers in `std`, and `latte strings <topic>` prints these in a
terminal (topics: analysis, anagrams, search, families).
