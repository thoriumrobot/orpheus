# Debug.Tool — the Loom call tracer
Every arm call with its arguments and result, as an expandable tree. Click a call in the
output to STEP into it; the break field keeps only subtrees rooted at that arm.

expr [field: expr=(fib 6)]  break [field: brk=dec]
[Trace it](run: debug $expr) [Trace with breakpoint](run: debug break=$brk $expr)

[Example: primes](run: debug (primes 20)) [Example: harmony parcels](run: debug break=nadd (harmony_alloc [400 [1200 [400 0]]] 1000 40))

Jetted arms (fast %name) run natively — the tracer shows the call, not their inner
reductions. CLI form: `latte debug --break ARM "<expr>"`.
