//! Host front-end for the data-intensive (DDIA) Latte libraries. The techniques
//! live in `lib/*.lat` (bloom, lsm, btree, vclock, crdt, lamport, chash, quorum,
//! wire, mapred, merkle); this driver links them, runs guided demos, and renders
//! Latte values readably (cords as text, lists with commas). `latte ddia [topic]`.

use crate::knot::{Knot, N};
use crate::latte;

fn run(expr: &str) -> Result<N, String> {
    let libs = latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    latte::run_with_libs(expr, &refs)
}

/// Render an atom: a multi-byte printable atom is a cord (shown %text); anything
/// else is its natural value. (So %val/%absent/%n1 read as text, counts as ints.)
fn show_atom(a: &crate::atom::Atom) -> String {
    let b = a.bytes_le();
    if b.len() >= 2 && b.iter().all(|&c| (0x20..0x7f).contains(&c)) {
        format!("%{}", String::from_utf8_lossy(b))
    } else {
        a.to_u128().map(|x| x.to_string()).unwrap_or_else(|| "<big>".into())
    }
}

/// Render a Knot: 0-terminated cells as [a, b, c]; improper pairs as [a | b].
fn show(n: &N) -> String {
    match &**n {
        Knot::Atom(a) => show_atom(a),
        Knot::Cell(_, _) => {
            let mut items = Vec::new();
            let mut cur = n.clone();
            loop {
                match &*cur {
                    Knot::Cell(h, t) => {
                        items.push(show(h));
                        cur = t.clone();
                    }
                    Knot::Atom(a) if a.bytes_le().is_empty() => {
                        return format!("[{}]", items.join(", "));
                    }
                    Knot::Atom(_) => {
                        return format!("[{} | {}]", items.join(", "), show(&cur));
                    }
                }
            }
        }
    }
}

fn row(label: &str, expr: &str) {
    match run(expr) {
        Ok(v) => println!("  {:<34} {}", label, show(&v)),
        Err(e) => println!("  {:<34} ! {}", label, e),
    }
}

fn head(title: &str, blurb: &str) {
    println!("\n\x1b[1m{}\x1b[0m", title);
    println!("  {}", blurb);
}

pub fn cmd_ddia(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Designing Data-Intensive Applications — techniques in Latte (lib/*.lat)");
    if all {
        println!("topics: bloom lsm btree vclock crdt lamport chash quorum wire mapred merkle");
        println!("        mvcc hll cms stream raft db lookup acplan symbols stats findb");
    }

    if all || topic == "bloom" {
        head("Bloom filter (Ch.3)", "probabilistic membership: no false negatives, tunable false positives");
        let f = "(bloom_addall (bloom_make 128 4) [%apple [%banana [%cherry 0]]])";
        row("test apple (0=present)", &format!("(bloom_test {} %apple)", f));
        row("test cherry (0=present)", &format!("(bloom_test {} %cherry)", f));
        row("test grape (1=absent)", &format!("(bloom_test {} %grape)", f));
        row("bits set of 128", &format!("(bloom_count {})", f));
    }

    if all || topic == "lsm" {
        head("LSM-tree / SSTable (Ch.3)", "append to a memtable, flush sorted segments, compact, tombstone deletes");
        let s = "(lsm_del (lsm_put (lsm_put (lsm_put (lsm_put (lsm_put (lsm_new 2) %a 10) %b 20) %c 30) %d 40) %b 99) %c)";
        row("get b (overwrite wins)", &format!("(lsm_get {} %b)", s));
        row("get c (tombstoned)", &format!("(lsm_get {} %c)", s));
        row("get d", &format!("(lsm_get {} %d)", s));
        row("segments before compaction", &format!("(lsm_nsegs {})", s));
        row("segments after compaction", &format!("(lsm_nsegs (lsm_compact {}))", s));
        row("get c after compaction", &format!("(lsm_get (lsm_compact {}) %c)", s));
        row("live keys (merged view)", &format!("(lsm_keys {})", s));
        row("range scan [a,c]", &format!("(map (fn [e] -> (head e)) (lsm_range {} %a %c))", s));
        row("segment zone maps [min[max cnt]]", &format!("(lsm_segmeta {})", s));
        row("runs scanned for a (zone-skip)", &format!("(lsm_probed {} %a)", s));
        row("runs scanned for zz (none: out of range)", &format!("(lsm_probed {} %zz)", s));
    }

    if all || topic == "btree" {
        head("B-tree (Ch.3)", "balanced high-fanout index, updated in place, O(log n) reads — the read-optimized family");
        let p = "[[%mm 1] [[%dd 2] [[%tt 3] [[%aa 4] [[%gg 5] [[%pp 6] [[%cc 7] [[%kk 8] [[%ww 9] [[%bb 10] [[%ff 11] [[%zz 12] 0]]]]]]]]]]]]";
        let t = format!("(bt_build {} 2)", p);
        row("in-order scan (sorted)", &format!("(map (fn [e] -> (head e)) (bt_inorder {}))", t));
        row("height (12 keys, t=2)", &format!("(bt_height {})", t));
        row("search aa", &format!("(bt_search {} %aa)", t));
        row("search qq (absent)", &format!("(bt_search {} %qq)", t));
        row("range scan [cc,mm]", &format!("(map (fn [e] -> (head e)) (bt_range {} %cc %mm))", t));
        row("min / max key", &format!("[ (head (bt_min {})) (head (bt_max {})) ]", t, t));
        row("delete mm, then search mm", &format!("(bt_search (bt_delete {} %mm 2) %mm)", t));
        row("count after deleting mm (was 12)", &format!("(bt_count (bt_delete {} %mm 2))", t));
    }

    if all || topic == "vclock" {
        head("Vector clocks (Ch.5)", "track causality; detect concurrent writes (conflicts) instead of losing them");
        let a = "(vc_inc (vc_inc (vc_new 0) %a) %a)";
        let b = "(vc_inc (vc_new 0) %b)";
        row("compare {a:2} vs {a:1}", &format!("(vc_compare {} (vc_inc (vc_new 0) %a))", a));
        row("compare {a:2} vs {b:1}", &format!("(vc_compare {} {})", a, b));
        row("merge {a:2} {b:1}", &format!("(vc_merge {} {})", a, b));
    }

    if all || topic == "crdt" {
        head("CRDTs (Ch.5)", "merge is commutative/associative/idempotent -> strong eventual consistency");
        let r1 = "(gc_inc (gc_new 0) %a 5)";
        let r2 = "(gc_inc (gc_new 0) %b 7)";
        row("G-Counter merge(R1,R2)", &format!("(gc_value (gc_merge {} {}))", r1, r2));
        row("G-Counter merge(R2,R1)", &format!("(gc_value (gc_merge {} {}))", r2, r1));
        row("PN-Counter +10 -3", "(pn_value (pn_dec (pn_inc (pn_new 0) %a 10) %b 3))");
        row("LWW-Register (ts9 wins)", "(lww_value (lww_merge (lww_set 5 %red %n1) (lww_set 9 %blue %n2)))");
        // OR-Set: a concurrent add survives a remove that never observed it
        let rmv = "(or_remove (or_add (or_new 0) %x [%n1 1]) %x)";
        let cadd = "(or_add (or_new 0) %x [%n2 1])";
        row("OR-Set add+remove on R1 (1=gone)", &format!("(or_has {} %x)", rmv));
        row("OR-Set merge w/ concurrent add", &format!("(or_has (or_merge {} {}) %x)", rmv, cadd));
        // MV-Register: concurrent writes are kept as siblings
        let wa = "(mv_set (mv_new 0) %red (vc_inc (vc_new 0) %a))";
        let wb = "(mv_set (mv_new 0) %blue (vc_inc (vc_new 0) %b))";
        row("MV-Register siblings", &format!("(mv_values (mv_merge {} {}))", wa, wb));
    }

    if all || topic == "lamport" {
        head("Lamport clocks (Ch.8-9)", "logical timestamps consistent with causality; total order via (ts, node)");
        row("recv(local=2, msg=5)", "(lam_recv 2 5)");
        row("total order of events", "(lam_order [ [3 %a] [ [1 %b] [ [3 %z] [ [2 %c] 0 ] ] ] ])");
    }

    if all || topic == "chash" {
        head("Consistent hashing (Ch.6)", "ring + virtual nodes; adding/removing a node moves ~1/N of keys, not all");
        let keys = "[%k1 [%k2 [%k3 [%k4 [%k5 [%k6 [%k7 [%k8 [%k9 [%k10 [%k11 [%k12 [%k13 [%k14 [%k15 [%k16 [%k17 [%k18 [%k19 [%k20 0]]]]]]]]]]]]]]]]]]]]";
        let r3 = "(ch_build [%n1 [%n2 [%n3 0]]] 16 65536)";
        let a3 = format!("(ch_assign {} {} 65536)", r3, keys);
        let a4 = format!("(ch_assign (ch_addnode {} %n4 16 65536) {} 65536)", r3, keys);
        let arm = format!("(ch_assign (ch_removenode {} %n2) {} 65536)", r3, keys);
        row("of 20 keys, moved adding n4", &format!("(ch_moved {} {})", a3, a4));
        row("of 20 keys, moved dropping n2", &format!("(ch_moved {} {})", a3, arm));
        row("replica list for k1 (3 nodes)", &format!("(ch_replicas {} %k1 3 65536)", r3));
        row("rendezvous (HRW) owner of k1", "(rv_lookup [%n1 [%n2 [%n3 0]]] %k1)");
        row("HRW top-2 replicas for k1", "(rv_topn [%n1 [%n2 [%n3 0]]] %k1 2)");
    }

    if all || topic == "quorum" {
        head("Quorum replication (Ch.5)", "W+R>N forces read/write overlap, so reads see the latest write");
        row("overlap N=3 W=2 R=2 (0=yes)", "(q_overlap 3 2 2)");
        row("overlap N=3 W=1 R=1 (1=no)", "(q_overlap 3 1 1)");
        row("strong (W2,R2) read", "(q_readval (q_write (q_init 3) 2 1 %hi) 2)");
        row("weak (W1,R1) read (stale)", "(q_readval (q_write (q_init 3) 1 1 %hi) 1)");
    }

    if all || topic == "wire" {
        head("Encoding & evolution (Ch.4)", "varint (LEB128) + tagged records: skip unknown fields, default missing ones");
        row("varint 300", "(w_varint 300)");
        row("roundtrip 1000000", "(w_unvarint (w_varint 1000000))");
        let rec = "[[1 100] [[2 200] [[5 999] 0]]]";
        let rs = "[[1 0] [[2 0] [[3 7] 0]]]";
        row("reader view (3 defaults to 7)", &format!("(w_read {} {})", rec, rs));
        row("unknown tags skipped", &format!("(w_unknown {} {})", rec, rs));
        row("zigzag -1 / +1 / -2", "[ (w_zigzag [1 1]) [ (w_zigzag [0 1]) (w_zigzag [1 2]) ] ]");
        // length-delimited framing: a reader round-trips a field whose tag it doesn't know
        let frame = "[[1 [42 0]] [[9 [7 [7 0]]] 0]]";
        row("framed record decoded", &format!("(w_decrec (w_encrec {}))", frame));
    }

    if all || topic == "mapred" {
        head("MapReduce (Ch.10)", "map -> shuffle (group by key) -> reduce; the restartable batch model");
        row("word count", "(mr_wordcount [[%the [%cat 0]] [[%the [%dog 0]] [[%the [%cat [%sat 0]]] 0]]])");
    }

    if all || topic == "merkle" {
        head("Merkle trees (Ch.5)", "one root hash to check if replicas match; descend to find exactly which keys differ");
        let a = "(mk_build [[%a 1] [[%b 2] [[%c 3] [[%d 4] 0]]]])";
        let c = "(mk_build [[%a 1] [[%b 2] [[%c 99] [[%d 4] 0]]]])";
        row("roots equal? identical (0)", &format!("(mk_equal {} {})", a, a));
        row("roots equal? one differs (1)", &format!("(mk_equal {} {})", a, c));
        row("keys that differ", &format!("(mk_diffkeys {} {})", a, c));
    }

    if all || topic == "mvcc" {
        head("MVCC / snapshot isolation (Ch.7)", "versioned writes; a transaction reads its start-time snapshot; write-write conflicts abort");
        // x=1 committed at ts5, then x=2 committed at ts15
        let store = "(mvcc_commit (mvcc_commit (mvcc_new 0) %x 1 5) %x 2 15)";
        row("read x @snapshot 10 (sees v1)", &format!("(mvcc_read {} %x 10)", store));
        row("read x @snapshot 20 (sees v2)", &format!("(mvcc_read {} %x 20)", store));
        row("tx writing x [start10,commit18]", &format!("(head (mvcc_try {} [[%x 9] 0] 10 18))", store));
        row("tx writing y [start10,commit18]", &format!("(head (mvcc_try {} [[%y 9] 0] 10 18))", store));
        // write skew: x=y=1; T1 sets x=0 (commits ts10); T2 read {x,y}, sets y=0 (commit ts15)
        let s0 = "(mvcc_commit (mvcc_commit (mvcc_new 0) %x 1 1) %y 1 1)";
        let after_t1 = format!("(tail (mvcc_ssi_try {} [%x [%y 0]] [[%x 0] 0] 5 10))", s0);
        row("write skew under plain SI: T2 commits", &format!("(head (mvcc_try {} [[%y 0] 0] 5 15))", after_t1));
        row("write skew under SSI: T2 aborts", &format!("(head (mvcc_ssi_try {} [%x [%y 0]] [[%y 0] 0] 5 15))", after_t1));
    }

    if all || topic == "hll" {
        head("HyperLogLog (analytics)", "count distinct in tiny fixed memory; ~1.04/sqrt(m) error (here m=256)");
        let ks = "(map (fn [i] -> (cat %k (numtext i))) (range 400))";
        let h = format!("(hll_addall (hll_new 8) {})", ks);
        row("estimate of 400 distinct", &format!("(hll_count {})", h));
        row("idempotent (add the 400 again)", &format!("(hll_count (hll_addall {} {}))", h, ks));
        row("empty registers (fill gauge)", &format!("(hll_zeros {})", h));
    }

    if all || topic == "cms" {
        head("Count-Min Sketch (analytics)", "frequency estimate in sublinear space; never under-counts (min over d rows)");
        let s = "(cms_addall (cms_new 4 64) [%a [%a [%a [%a [%a [%b [%b [%c 0]]]]]]]])";
        row("count a (added 5x)", &format!("(cms_count {} %a)", s));
        row("count b (added 2x)", &format!("(cms_count {} %b)", s));
        row("count z (never added)", &format!("(cms_count {} %z)", s));
    }

    if all || topic == "stream" {
        head("Stream windowing (Ch.11)", "tumbling windows over event time; a watermark separates on-time from late events");
        let ev = "[[3 5] [[7 2] [[12 8] [[15 1] [[25 4] 0]]]]]";
        row("tumbling sums (window=10)", &format!("(stream_sums {} 10)", ev));
        row("tumbling counts (window=10)", &format!("(stream_counts {} 10)", ev));
        row("late events before watermark 10", &format!("(stream_late {} 10)", ev));
        row("sliding sums (size10, step5)", &format!("(stream_slide_sum {} 10 5)", ev));
    }

    if all || topic == "raft" {
        head("Raft consensus (Ch.9)", "election restriction + log matching + commit-on-majority — the safety core");
        let log = "[[1 %a] [[1 %b] [[2 %c] 0]]]";
        row("up-to-date? cand(t1,i5) vs me(t2,i1)", "(raft_uptodate 1 5 2 1)");
        row("grant vote (fresh, log ok)", "(raft_grantvote 3 %none %n2 3 2 3 2 3)");
        row("append @prev(3,t2): log matches", &format!("(head (raft_append {} 3 2 [[3 %d] 0]))", log));
        row("append @prev(3,t9): mismatch", &format!("(head (raft_append {} 3 9 [[3 %d] 0]))", log));
        row("commit idx, match [3 3 2 1 1] n5", "(raft_commit [3 [3 [2 [1 [1 0]]]]] 5)");
    }

    if all || topic == "db" {
        head("A composed database (Ch.3–9)",
             "LSM + WAL recovery + Bloom + secondary index + MVCC snapshots + SSI + leaderless replication + 2PC");
        row("get u1 (point read)", "(db_get (db_sample 0) %u1)");
        row("get u9 (Bloom -> absent, store skipped)", "(db_get (db_sample 0) %u9)");
        row("query city=nyc (secondary index)", "(map (fn [r] -> (head r)) (db_query (db_sample 0) %nyc))");
        row("update u1 nyc->sfo, re-query nyc", "(map (fn [r] -> (head r)) (db_query (db_put (db_sample 0) %u1 [[1 %alice] [[2 %sfo] 0]]) %nyc))");
        row("range scan [u1,u2]", "(map (fn [e] -> (head e)) (db_range (db_sample 0) %u1 %u2))");
        // schema evolution (Ch.4): an old row read through a schema that expects a new field
        let rs = "[[1 0] [[2 0] [[3 %zz] 0]]]";
        row("schema evolution: field 3 defaults",
            &format!("(db_get (db_put (db_open 2 {} 4) %u1 [[1 %alice] [[2 %nyc] 0]]) %u1)", rs));
        // MVCC: a write adds a version; an old snapshot still sees the old value
        let bumped = "(db_put (db_sample 0) %u1 [[1 %alice2] [[2 %la] 0]])";
        row("MVCC time-travel: read u1 at snapshot ts=1", &format!("(db_getat {} %u1 1)", bumped));
        row("MVCC: read u1 at the latest snapshot", &format!("(db_get {} %u1)", bumped));
        // durability: the write-ahead log replays onto a fresh db
        row("WAL length after the sample's writes", "(len (db_wal (db_sample 0)))");
        row("crash recovery: replay WAL, read u3", "(db_get (db_recover (db_open 2 0 4) (db_wal (db_sample 0))) %u3)");
        // serializable transactions
        row("snapshot read ignores a later write",
            "(let d = (db_sample 0) in let s = (db_begin d) in (db_txget (db_put d %u1 [[1 %changed] 0]) s %u1))");
        row("txn commit (read set still valid)",
            "(head (db_commit (db_sample 0) (db_begin (db_sample 0)) [%u1 0] [[%u4 [[1 %dave] 0]] 0]))");
        row("write skew -> abort (u1 changed)",
            "(let snap = (db_sample 0) in let cur = (db_put snap %u1 [[1 %alx] 0]) in (head (db_commit cur (db_begin snap) [%u1 0] [[%u2 [[1 %bz] 0]] 0])))");
        // partitioning + replication + atomic commit
        row("partition route u1 (3-node ring)", "(db_route (ch_build [%n1 [%n2 [%n3 0]]] 16 65536) %u1 65536)");
        row("leaderless: W+R>N is strong? (0=yes)", "(clu_strong (clu_sample 0))");
        row("quorum read after a write", "(clu_get (clu_put (clu_sample 0) %k %v1 %n0) %k)");
        row("two concurrent blind writes -> siblings",
            "(clu_get (clu_putblind (clu_putblind (clu_sample 0) %k %va %n0) %k %vb %n1) %k)");
        row("a coordinated write collapses the siblings",
            "(clu_get (clu_put (clu_putblind (clu_putblind (clu_sample 0) %k %va %n0) %k %vb %n1) %k %vc %n0) %k)");
        row("2PC: all shards vote yes -> commit", "(head (tpc_run [%sA [%sB [%sC 0]]] (fn [s] -> %yes)))");
        row("2PC: one shard votes no -> abort",
            "(head (tpc_run [%sA [%sB [%sC 0]]] (fn [s] -> if (s == %sB) then %no else %yes)))");
    }

    if all || topic == "lookup" {
        head("Definition-lookup tool",
             "highlight a function name; lk_lookup returns its doc comment + definition from the module source");
        row("lk_lookup sample for b (sig + body)", "(lk_lookup (lk_sample 0) %b)");
        row("lk_lookup sample for a (doc + def)", "(lk_lookup (lk_sample 0) %a)");
        row("missing name -> %none", "(lk_lookup (lk_sample 0) %z)");
    }

    if all || topic == "acplan" {
        head("Accountable planning (Materialist Economics, Part 3)",
             "quadratic vote -> final demand -> Leontief plan -> labour values, vouchers, accountability (built on plan)");
        // 2. democratic job creation via quadratic voting
        row("QV cost of ballot [0 0 2 4] (4+16)", "(qv_cost [0 [0 [2 [4 0]]]])");
        row("affordable on a 30-credit budget? (0=yes)", "(qv_afford 30 [0 [0 [2 [4 0]]]])");
        row("tally three ballots -> votes/category", "(qv_tally (ap_ballots 0))");
        row("final demand vector (scaled x1000)", "(ap_demand (qv_tally (ap_ballots 0)))");
        // 3. accountable planning: feed the vote into the I/O computation
        row("balanced targets iron/coal/corn/bread", "(ap_targetsof (ap_sample 0))");
        row("turnpike residual x-(y+Ax) (~0 = converged)",
            "(ap_residual (ap_tech 0) (ap_targetsof (ap_sample 0)) (ap_demandof (ap_sample 0)))");
        row("labour values per unit (scaled)", "(ap_valuesof (ap_sample 0))");
        row("total social labour the plan needs", "(ap_labourof (ap_sample 0))");
        // 6. intermediate goods at labour-time cost
        row("embodied labour in 5 corn (no margin)", "(ap_embodied (ap_valuesof (ap_sample 0)) 2 5000)");
        row("deliver 5 corn: consumer debited to",
            "(led_bal (ap_deliver (vou_issue 0 %housing 10000) %housing %works (ap_valuesof (ap_sample 0)) 2 5000) %housing)");
        // 5. labour vouchers (non-circulating) and the MELT price bridge
        row("voucher: issue 8h, redeem 3h -> held", "(vou_held (vou_redeem (vou_issue 0 %ann 8000) %ann 3000) %ann)");
        row("predicted prices = values x MELT",
            "(ap_prices (ap_valuesof (ap_sample 0)) (ap_melt 36400000 (mul (ap_labourof (ap_sample 0)) 1000)))");
        // 3/7/8. accountability: which voted-for goods are under-supplied
        row("under-supplied beyond margin (bread short)",
            "(ap_undersupplied (ap_targetsof (ap_sample 0)) [1041 [2908 [15000 [10000 0]]]] 1000)");
    }

    if all || topic == "symbols" {
        head("A database-backed symbol index (code navigation on db)",
             "every arm is a record [name module arity] indexed by name; who-defines / shadowing / MVCC redefinition history become db ops");
        row("modules that define `dot` (secondary-index query)", "(sy_modulesof (sy_sample 0) %dot)");
        row("how many define `dot`", "(sy_count (sy_sample 0) %dot)");
        row("is `dot` shadowed in the flat scope? (0=yes)", "(sy_shadowed (sy_sample 0) %dot)");
        row("is `gross` shadowed? (1=no, single definer)", "(sy_shadowed (sy_sample 0) %gross)");
        row("arity recorded for plan.dot", "(sy_arityin (sy_sample 0) %plan %dot)");
        // MVCC: recompiling an arm at runtime is just another versioned write
        row("redefine plan.dot, then count its revisions", "(sy_revisions (sy_redefine (sy_sample 0) %plan %dot 9) %plan %dot)");
    }

    if all || topic == "stats" {
        head("A statistics library on signed fixed-point lists (lib/stats.lat)",
             "mean/variance/std, median/quantile, covariance/correlation, least-squares regression, z-scores — the shared home ta/fin/findb route through");
        row("mean of the sample [2 4 4 4 5 5 7 9]", "(st_mean (st_sample 0))");
        row("population standard deviation (= 2)", "(st_std (st_sample 0))");
        row("median (= 4.5)", "(st_median (st_sample 0))");
        row("first quartile (type-7 interpolation)", "(st_quantile (st_sample 0) (ndiv (nint 1) (nint 4)))");
        // a perfectly linear series y = 2x + 1: correlation 1, slope 2, intercept 1
        let xs = "[ (nint 1) [ (nint 2) [ (nint 3) [ (nint 4) 0 ] ] ] ]";
        let ys = "[ (nint 3) [ (nint 5) [ (nint 7) [ (nint 9) 0 ] ] ] ]";
        row("correlation of x and 2x+1 (= 1.0)", &format!("(st_corr {} {})", xs, ys));
        row("least-squares line [slope intercept]", &format!("(st_linreg {} {})", xs, ys));
        row("predict that line at x = 10 (= 21)", &format!("(st_predict (st_linreg {} {}) (nint 10))", xs, ys));
    }

    if all || topic == "findb" {
        head("Financial price data in the database (lib/findb.lat on lib/db.lat)",
             "a price window is stored as records, read back for an SVG sparkline (visualization) and a lag-1 regression (training), summarised with stats.lat");
        row("up moves in the 12-bar sample window", "(fd_updays_of (fd_prices 0))");
        row("[mean std] of the stored closes", "(fd_stats_of (fd_prices 0))");
        row("realized volatility (std of returns)", "(fd_vol_of (fd_prices 0))");
        row("lag-1 model [slope intercept correlation]", "(fd_fit_of (fd_prices 0))");
        // MVCC: correcting a stored bar keeps the previous reading in its history
        row("a corrected bar keeps 2 versions in history", "(len (fd_history (fd_correct (fd_sample 0) 0 (npos 101000) 0) 0))");
    }

    if all {
        println!("\nEach technique is a Latte module in lib/ (namespaced: dh_/bloom_/lsm_/bt_/vc_/");
        println!("gc_,or_,mv_/lam_/ch_,rv_/q_/w_/mr_/mk_/mvcc_/hll_/cms_/stream_/raft_).");
        println!("See docs/data-intensive.md for the full interview guide.");
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    fn n(expr: &str) -> u128 {
        run(expr).unwrap().as_atom().unwrap().to_u128().unwrap()
    }

    #[test]
    fn bloom_no_false_negatives() {
        let f = "(bloom_addall (bloom_make 128 4) [%apple [%banana [%cherry 0]]])";
        assert_eq!(n(&format!("(bloom_test {} %apple)", f)), 0); // present
        assert_eq!(n(&format!("(bloom_test {} %banana)", f)), 0);
        assert_eq!(n(&format!("(bloom_test {} %grape)", f)), 1); // absent
    }

    #[test]
    fn db_get_query_and_bloom_skip() {
        let d = "(db_sample 0)";
        // primary read returns the stored record under the %rec tag
        assert_eq!(run(&format!("(head (db_get {} %u1))", d)).unwrap(), crate::knot::cord("rec"));
        // a key never inserted is reported absent and the Bloom filter skips the store read
        assert_eq!(run(&format!("(db_get {} %u9)", d)).unwrap(), crate::knot::cord("absent"));
        assert_eq!(n(&format!("(db_skipped {} %u9)", d)), 1);
        // the secondary index on the city field returns both nyc users
        assert_eq!(n(&format!("(len (db_query {} %nyc))", d)), 2);
        assert_eq!(n(&format!("(len (db_query {} %sfo))", d)), 1);
    }

    #[test]
    fn db_secondary_index_follows_updates() {
        // moving u1 from nyc to sfo must remove it from the nyc posting list and add it to sfo
        let d = "(db_put (db_sample 0) %u1 [[1 %alice] [[2 %sfo] 0]])";
        assert_eq!(n(&format!("(len (db_query {} %nyc))", d)), 1); // only u3 left in nyc
        assert_eq!(n(&format!("(len (db_query {} %sfo))", d)), 2); // u2 and now u1
    }

    #[test]
    fn db_mvcc_snapshot_time_travel() {
        // u1 is written at ts=1, then overwritten later; an old snapshot still sees the old value
        let d = "(db_put (db_sample 0) %u1 [[1 %alice2] 0])";
        // at snapshot ts=1, u1's name field is the original alice
        assert_eq!(run(&format!("(tail (w_find (tail (db_getat {} %u1 1)) 1))", d)).unwrap(),
                   crate::knot::cord("alice"));
        // at the latest snapshot it is alice2
        assert_eq!(run(&format!("(tail (w_find (tail (db_get {} %u1)) 1))", d)).unwrap(),
                   crate::knot::cord("alice2"));
        // the version chain records both versions
        assert_eq!(n(&format!("(len (db_history {} %u1))", d)), 2);
    }

    #[test]
    fn db_wal_recovery_reproduces_state() {
        // replaying the write-ahead log onto a fresh db reproduces reads, deletes included
        let recovered = "(db_recover (db_open 2 0 4) (db_wal (db_delete (db_sample 0) %u1)))";
        assert_eq!(run(&format!("(db_get {} %u1)", recovered)).unwrap(), crate::knot::cord("absent"));
        assert_eq!(run(&format!("(head (db_get {} %u3))", recovered)).unwrap(), crate::knot::cord("rec"));
        // every mutation was logged: 3 puts + 1 delete
        assert_eq!(n("(len (db_wal (db_delete (db_sample 0) %u1)))"), 4);
    }

    #[test]
    fn db_transactions_prevent_write_skew() {
        // a transaction whose read set still holds commits
        let ok = "(head (db_commit (db_sample 0) (db_begin (db_sample 0)) [%u1 0] [[%u4 [[1 %dave] 0]] 0]))";
        assert_eq!(run(ok).unwrap(), crate::knot::cord("commit"));
        // a transaction that read u1 aborts once u1 has a newer committed version (write skew)
        let skew = "(let snap = (db_sample 0) in let cur = (db_put snap %u1 [[1 %alx] 0]) in \
                     (head (db_commit cur (db_begin snap) [%u1 0] [[%u2 [[1 %bz] 0]] 0])))";
        assert_eq!(run(skew).unwrap(), crate::knot::cord("abort"));
    }

    #[test]
    fn db_leaderless_quorum_and_siblings() {
        // W+R>N gives a consistent quorum read
        assert_eq!(n("(clu_strong (clu_sample 0))"), 0);
        // a write then a quorum read returns that value
        assert_eq!(run("(head (clu_get (clu_put (clu_sample 0) %k %v1 %n0) %k))").unwrap(),
                   crate::knot::cord("val"));
        // two concurrent blind writes from different nodes surface as two siblings
        let sib = "(clu_get (clu_putblind (clu_putblind (clu_sample 0) %k %va %n0) %k %vb %n1) %k)";
        assert_eq!(run(&format!("(head {})", sib)).unwrap(), crate::knot::cord("siblings"));
        assert_eq!(n(&format!("(len (tail {}))", sib)), 2);
        // a later coordinated write supersedes both, collapsing the conflict
        let fixed = "(clu_get (clu_put (clu_putblind (clu_putblind (clu_sample 0) %k %va %n0) %k %vb %n1) %k %vc %n0) %k)";
        assert_eq!(run(&format!("(head {})", fixed)).unwrap(), crate::knot::cord("val"));
        // read repair heals replicas without inventing a spurious sibling
        let repaired = "(clu_get (clu_repair (clu_put (clu_sample 0) %k %v1 %n0) %k) %k)";
        assert_eq!(run(&format!("(head {})", repaired)).unwrap(), crate::knot::cord("val"));
    }

    #[test]
    fn db_two_phase_commit_is_atomic() {
        // unanimous yes commits; a single no aborts the whole transaction
        assert_eq!(run("(head (tpc_run [%a [%b [%c 0]]] (fn [s] -> %yes)))").unwrap(),
                   crate::knot::cord("commit"));
        assert_eq!(run("(head (tpc_run [%a [%b [%c 0]]] (fn [s] -> if (s == %b) then %no else %yes)))").unwrap(),
                   crate::knot::cord("abort"));
    }

    #[test]
    fn db_schema_evolution_on_read() {
        // a row written without field 3, read through a schema that expects it, gets the default
        let d = "(db_put (db_open 2 [[1 0] [[2 0] [[3 %zz] 0]]] 4) %u1 [[1 %alice] [[2 %nyc] 0]])";
        assert_eq!(run(&format!("(tail (w_find (tail (db_get {} %u1)) 3))", d)).unwrap(),
                   crate::knot::cord("zz"));
        // and the fields the row does carry are untouched
        assert_eq!(run(&format!("(tail (w_find (tail (db_get {} %u1)) 1))", d)).unwrap(),
                   crate::knot::cord("alice"));
    }

    #[test]
    fn db_range_scan_and_compaction() {
        // a primary-key range scan rides the LSM's sorted order
        assert_eq!(n("(len (db_range (db_sample 0) %u1 %u2))"), 2);
        // reads still resolve after the store and index are compacted
        assert_eq!(run("(head (db_get (db_compact (db_sample 0)) %u3))").unwrap(),
                   crate::knot::cord("rec"));
        assert_eq!(n("(len (db_query (db_compact (db_sample 0)) %nyc))"), 2);
    }

    #[test]
    fn db_snapshot_read_is_stable() {
        // db_txget reads through the transaction's snapshot, so a write committed after
        // the snapshot is invisible to it (repeatable read / snapshot isolation)
        let e = "(let d = (db_sample 0) in let snap = (db_begin d) in \
                  let d2 = (db_put d %u1 [[1 %changed] 0]) in (tail (w_find (tail (db_txget d2 snap %u1)) 1)))";
        assert_eq!(run(e).unwrap(), crate::knot::cord("alice"));
    }

    #[test]
    fn lookup_finds_definition_with_comment() {
        let s = "(lk_sample 0)";
        // b has a signature line + an indented body line -> two lines, no preceding comment
        assert_eq!(n(&format!("(len (lk_lookup {} %b))", s)), 2);
        // a is preceded by a doc comment -> the comment is included
        assert_eq!(n(&format!("(len (lk_lookup {} %a))", s)), 2);
        assert_eq!(run(&format!("(lk_iscomment (head (lk_lookup {} %a)))", s)).unwrap(),
                   crate::knot::num(0)); // first returned line is the comment
        // a missing name yields %none
        assert_eq!(run(&format!("(lk_lookup {} %zzz)", s)).unwrap(), crate::knot::cord("none"));
    }

    #[test]
    fn acplan_quadratic_voting_aggregates_demand() {
        assert_eq!(n("(qv_cost [0 [0 [2 [4 0]]]])"), 20);          // 2^2 + 4^2
        assert_eq!(n("(qv_afford 30 [0 [0 [2 [4 0]]]])"), 0);       // affordable
        assert_eq!(n("(qv_afford 15 [0 [0 [2 [4 0]]]])"), 1);       // not affordable
        // three ballots tally to corn=6, bread=12 votes
        assert_eq!(n("(nth (qv_tally (ap_ballots 0)) 2)"), 6);
        assert_eq!(n("(nth (qv_tally (ap_ballots 0)) 3)"), 12);
        // the tally becomes the final-demand vector (scaled into fixed point)
        assert_eq!(n("(nth (ap_demand (qv_tally (ap_ballots 0))) 3)"), 12000);
    }

    #[test]
    fn acplan_plan_converges_and_balances() {
        // the contractive affine transform reaches its fixed point: residual is 0
        assert_eq!(n("(ap_residual (ap_tech 0) (ap_targetsof (ap_sample 0)) (ap_demandof (ap_sample 0)))"), 0);
        // bread is a pure final good, so its target equals the voted demand (12 units)
        assert_eq!(n("(nth (ap_targetsof (ap_sample 0)) 3)"), 12000);
        // the plan mobilises a positive amount of social labour
        assert!(n("(ap_labourof (ap_sample 0))") > 0);
        // the invariant Σ v·y = Σ l·x holds to within fixed-point rounding (< 0.1%)
        let vy = n("(div (ap_dot (ap_valuesof (ap_sample 0)) (ap_demandof (ap_sample 0))) 1000)");
        let lx = n("(ap_labourof (ap_sample 0))");
        let diff = if vy > lx { vy - lx } else { lx - vy };
        assert!(diff * 1000 < lx);
    }

    #[test]
    fn acplan_labour_ledger_conserves_at_cost() {
        // delivering an intermediate good moves embodied labour from consumer to producer
        // with NO margin, so the two balances still sum to the consumer's original budget
        let led = "(ap_deliver (vou_issue 0 %housing 10000) %housing %works (ap_valuesof (ap_sample 0)) 2 5000)";
        let hz = n(&format!("(led_bal {} %housing)", led));
        let wz = n(&format!("(led_bal {} %works)", led));
        assert_eq!(hz + wz, 10000);
        assert!(wz > 0);
        assert_eq!(n("(ap_cheaper 1100 1147)"), 0); // the cheaper facility is chosen
    }

    #[test]
    fn acplan_vouchers_and_accountability() {
        // a labour voucher is issued, partly redeemed, and the rest remains outstanding
        assert_eq!(n("(vou_held (vou_redeem (vou_issue 0 %ann 8000) %ann 3000) %ann)"), 5000);
        assert_eq!(n("(vou_total (vou_issue (vou_issue 0 %ann 8000) %bo 6000))"), 14000);
        // when production matches the plan, every demanded good is met
        assert_eq!(n("(ap_met (ap_targetsof (ap_sample 0)) (ap_targetsof (ap_sample 0)))"), 0);
        // bread (index 3) short beyond the margin is flagged (the collectivisation trigger)
        assert_eq!(n("(head (ap_undersupplied (ap_targetsof (ap_sample 0)) [1041 [2908 [15000 [10000 0]]]] 1000))"), 3);
    }

    #[test]
    fn symbols_index_finds_definers_and_shadowing() {
        // `dot` is defined in three modules; the secondary-index query returns all three
        assert_eq!(n("(sy_count (sy_sample 0) %dot)"), 3);
        assert_eq!(n("(sy_shadowed (sy_sample 0) %dot)"), 0);            // 0 = shadowed
        assert_eq!(n("(sy_shadowed (sy_sample 0) %gross)"), 1);         // single definer, not shadowed
        assert_eq!(n("(sy_count (sy_sample 0) %values)"), 2);
        // a name nothing defines has no definers
        assert_eq!(n("(sy_count (sy_sample 0) %nosuchname)"), 0);
        // the modules of `dot` include plan (membership, regardless of posting-list order)
        assert_eq!(run("(any (fn [m] -> (m == %plan)) (sy_modulesof (sy_sample 0) %dot))").unwrap(),
                   crate::knot::num(0));
    }

    #[test]
    fn symbols_index_records_redefinition_history() {
        // the recorded arity is readable for a specific module's definition
        assert_eq!(n("(sy_arityin (sy_sample 0) %plan %gross)"), 3);
        // recompiling an arm (same module.name) appends an MVCC version
        assert_eq!(n("(sy_revisions (sy_sample 0) %plan %dot)"), 1);
        assert_eq!(n("(sy_revisions (sy_redefine (sy_sample 0) %plan %dot 9) %plan %dot)"), 2);
        // the newest version carries the new arity, the old one is still in history
        assert_eq!(n("(sy_arityin (sy_redefine (sy_sample 0) %plan %dot 9) %plan %dot)"), 9);
    }

    #[test]
    fn stats_descriptive_and_regression() {
        // sample [2 4 4 4 5 5 7 9]: mean 5.0, population std 2.0, median 4.5
        assert_eq!(n("(tail (st_mean (st_sample 0)))"), 5000);
        assert_eq!(n("(head (st_std (st_sample 0)))"), 0);      // sign 0 = positive
        assert_eq!(n("(tail (st_std (st_sample 0)))"), 2000);   // = 2.000
        assert_eq!(n("(tail (st_median (st_sample 0)))"), 4500);
        // a perfectly linear series y = 2x + 1: correlation 1, slope 2, intercept 1
        let xs = "[ (nint 1) [ (nint 2) [ (nint 3) [ (nint 4) 0 ] ] ] ]";
        let ys = "[ (nint 3) [ (nint 5) [ (nint 7) [ (nint 9) 0 ] ] ] ]";
        assert_eq!(n(&format!("(tail (st_corr {} {}))", xs, ys)), 1000);            // 1.0
        assert_eq!(n(&format!("(tail (nth (st_linreg {} {}) 0))", xs, ys)), 2000);  // slope 2
        assert_eq!(n(&format!("(tail (nth (st_linreg {} {}) 1))", xs, ys)), 1000);  // intercept 1
        assert_eq!(n(&format!("(tail (st_predict (st_linreg {} {}) (nint 10)))", xs, ys)), 21000); // 21
    }

    #[test]
    fn findb_store_read_visualize_train() {
        // the 12-bar sample window: 6 up-moves, mean close 108.000 read back from the store
        assert_eq!(n("(fd_updays_of (fd_prices 0))"), 6);
        assert_eq!(n("(tail (head (fd_stats_of (fd_prices 0))))"), 108000);
        // the lag-1 model is trained on returns derived from the stored closes (slope sign here is +)
        assert_eq!(n("(head (nth (fd_fit_of (fd_prices 0)) 0))"), 0);
        // VISUALIZATION: the sparkline is a valid [%svg ...] noun built from the stored series
        assert_eq!(run("(head (fd_spark (fd_sample 0) 12))").unwrap(), crate::knot::cord("svg"));
        // MVCC: correcting a stored bar keeps the previous reading in its version history
        assert_eq!(n("(len (fd_history (fd_correct (fd_sample 0) 0 (npos 101000) 0) 0))"), 2);
    }

    #[test]
    fn lsm_tombstone_and_compaction() {
        let s = "(lsm_del (lsm_put (lsm_put (lsm_put (lsm_new 2) %a 10) %b 20) %c 30) %b)";
        // tombstoned key reads as %absent (cord "absent")
        assert_eq!(run(&format!("(lsm_get {} %b)", s)).unwrap(), crate::knot::cord("absent"));
        // surviving key still readable after compaction
        assert_eq!(n(&format!("(tail (lsm_get (lsm_compact {}) %a))", s)), 10);
    }

    #[test]
    fn lsm_zone_map_skips_runs() {
        // zone-aware get matches the logical value, and probes fewer runs than exist
        let s = "(lsm_sample 0)";
        assert_eq!(n(&format!("(tail (lsm_get {} %apple))", s)), 1);
        assert_eq!(run(&format!("(lsm_get {} %elder)", s)).unwrap(), crate::knot::cord("absent")); // deleted
        let total = n(&format!("(add 1 (lsm_nsegs {}))", s));
        // a key in only one run is probed in fewer runs than the total
        assert!(n(&format!("(lsm_probed {} %apple)", s)) < total);
        // a key past every zone's max is skipped entirely (0 runs scanned)
        assert_eq!(n(&format!("(lsm_probed {} %zzz)", s)), 0);
    }

    #[test]
    fn btree_inorder_is_sorted_and_searchable() {
        let p = "[[%mm 1] [[%aa 2] [[%zz 3] [[%dd 4] [[%kk 5] 0]]]]]";
        let t = format!("(bt_build {} 2)", p);
        assert_eq!(n(&format!("(len (bt_inorder {}))", t)), 5);
        // bt_search returns [%val v]; pull the value for %zz
        assert_eq!(n(&format!("(tail (bt_search {} %zz))", t)), 3);
        assert_eq!(n(&format!("(bt_search {} %qq)", t)), 0); // absent
    }

    #[test]
    fn btree_delete_borrow_and_merge() {
        // delete every key from the sample tree one at a time -> each search misses, count drops
        for k in ["aa", "cc", "dd", "gg", "kk", "mm", "pp", "zz"] {
            assert_eq!(n(&format!("(bt_search (bt_delete (bt_sample 0) %{} 2) %{})", k, k)), 0);
            assert_eq!(n(&format!("(bt_count (bt_delete (bt_sample 0) %{} 2))", k)), 7);
        }
        // deleting an absent key leaves the tree unchanged
        assert_eq!(n("(bt_count (bt_delete (bt_sample 0) %qq 2))"), 8);
        // delete all eight in sequence -> empty tree
        let mut t = "(bt_sample 0)".to_string();
        for k in ["mm", "dd", "gg", "pp", "aa", "zz", "kk", "cc"] {
            t = format!("(bt_delete {} %{} 2)", t, k);
        }
        assert_eq!(n(&format!("(bt_count {})", t)), 0);
        // delete then reinsert is observable
        assert_eq!(n("(tail (bt_search (bt_insert (bt_delete (bt_sample 0) %mm 2) %mm 99 2) %mm))"), 99);
    }

    #[test]
    fn vclock_detects_concurrency() {
        let a = "(vc_inc (vc_new 0) %a)";
        let b = "(vc_inc (vc_new 0) %b)";
        assert_eq!(n(&format!("(vc_conflict {} {})", a, b)), 0); // concurrent -> conflict
        assert_eq!(
            run(&format!("(vc_compare {} {})", a, b)).unwrap(),
            crate::knot::cord("concurrent")
        );
    }

    #[test]
    fn crdt_merge_is_commutative() {
        let r1 = "(gc_inc (gc_new 0) %a 5)";
        let r2 = "(gc_inc (gc_new 0) %b 7)";
        assert_eq!(n(&format!("(gc_value (gc_merge {} {}))", r1, r2)), 12);
        assert_eq!(n(&format!("(gc_value (gc_merge {} {}))", r2, r1)), 12);
        // idempotent
        assert_eq!(n(&format!("(gc_value (gc_merge {} {}))", r1, r1)), 5);
    }

    #[test]
    fn consistent_hashing_moves_minimal_keys() {
        let keys = "[%k1 [%k2 [%k3 [%k4 [%k5 [%k6 [%k7 [%k8 0]]]]]]]]";
        let r3 = "(ch_build [%n1 [%n2 [%n3 0]]] 16 65536)";
        let a3 = format!("(ch_assign {} {} 65536)", r3, keys);
        let a4 = format!("(ch_assign (ch_addnode {} %n4 16 65536) {} 65536)", r3, keys);
        let moved = n(&format!("(ch_moved {} {})", a3, a4));
        assert!(moved < 8, "adding a 4th node should move well under all 8 keys, moved {}", moved);
    }

    #[test]
    fn quorum_overlap_rule() {
        assert_eq!(n("(q_overlap 3 2 2)"), 0); // W+R>N: strong
        assert_eq!(n("(q_overlap 3 1 1)"), 1); // W+R<=N: may be stale
        assert_eq!(run("(q_readval (q_write (q_init 3) 2 1 %hi) 2)").unwrap(), crate::knot::cord("hi"));
    }

    #[test]
    fn wire_varint_roundtrip_and_evolution() {
        assert_eq!(n("(w_unvarint (w_varint 1000000))"), 1000000);
        assert_eq!(n("(w_varlen 127)"), 1);
        assert_eq!(n("(w_varlen 128)"), 2);
        // a field unknown to the reader is skipped; missing field gets the default
        let rec = "[[1 100] [[2 200] [[5 999] 0]]]";
        let rs = "[[1 0] [[2 0] [[3 7] 0]]]";
        assert_eq!(n(&format!("(len (w_unknown {} {}))", rec, rs)), 1);
        assert_eq!(n(&format!("(tail (nth (w_read {} {}) 2))", rec, rs)), 7);
    }

    #[test]
    fn mapreduce_word_count() {
        let docs = "[[%the [%cat 0]] [[%the [%dog 0]] [[%the [%cat [%sat 0]]] 0]]]";
        // first group is "the" with count 3
        assert_eq!(n(&format!("(tail (head (mr_wordcount {})))", docs)), 3);
    }

    #[test]
    fn merkle_root_and_diff() {
        let a = "(mk_build [[%a 1] [[%b 2] [[%c 3] [[%d 4] 0]]]])";
        let same = a;
        let diff = "(mk_build [[%a 1] [[%b 2] [[%c 99] [[%d 4] 0]]]])";
        assert_eq!(n(&format!("(mk_equal {} {})", a, same)), 0); // identical
        assert_eq!(n(&format!("(mk_equal {} {})", a, diff)), 1); // differ
        // exactly one differing key, and it is %c
        assert_eq!(n(&format!("(len (mk_diffkeys {} {}))", a, diff)), 1);
        assert_eq!(run(&format!("(head (mk_diffkeys {} {}))", a, diff)).unwrap(), crate::knot::cord("c"));
    }

    #[test]
    fn orset_concurrent_add_beats_remove() {
        // R1 adds x then removes it (observing its own tag); R2 concurrently adds x.
        let rmv = "(or_remove (or_add (or_new 0) %x [%n1 1]) %x)";
        let cadd = "(or_add (or_new 0) %x [%n2 1])";
        assert_eq!(n(&format!("(or_has {} %x)", rmv)), 1); // gone on R1
        // after merge the concurrent (unobserved) add survives
        assert_eq!(n(&format!("(or_has (or_merge {} {}) %x)", rmv, cadd)), 0);
        assert_eq!(n(&format!("(or_has (or_merge {} {}) %x)", cadd, rmv)), 0); // commutative
    }

    #[test]
    fn mv_register_keeps_concurrent_siblings() {
        let wa = "(mv_set (mv_new 0) %red (vc_inc (vc_new 0) %a))";
        let wb = "(mv_set (mv_new 0) %blue (vc_inc (vc_new 0) %b))";
        assert_eq!(n(&format!("(len (mv_values (mv_merge {} {})))", wa, wb)), 2); // both kept
        // a write that dominates both collapses to one value
        let dom = format!("(mv_set (mv_merge {} {}) %green (vc_inc (vc_inc (vc_inc (vc_new 0) %a) %b) %c))", wa, wb);
        assert_eq!(n(&format!("(len (mv_values {}))", dom)), 1);
    }

    #[test]
    fn chash_replicas_and_rendezvous() {
        let r3 = "(ch_build [%n1 [%n2 [%n3 0]]] 16 65536)";
        assert_eq!(n(&format!("(len (ch_replicas {} %k1 3 65536))", r3)), 3); // 3 distinct replicas
        // HRW: the top-1 of the ranked set is exactly the single-owner lookup
        let nodes = "[%n1 [%n2 [%n3 0]]]";
        assert_eq!(n(&format!("((head (rv_topn {} %k1 1)) == (rv_lookup {} %k1))", nodes, nodes)), 0);
    }

    #[test]
    fn lsm_and_btree_range_scans() {
        let s = "(lsm_put (lsm_put (lsm_put (lsm_put (lsm_new 2) %a 1) %c 3) %e 5) %g 7)";
        assert_eq!(n(&format!("(len (lsm_range {} %b %f))", s)), 2); // c, e
        let p = "[[%mm 1] [[%dd 2] [[%aa 3] [[%gg 4] [[%cc 5] 0]]]]]";
        let t = format!("(bt_build {} 2)", p);
        assert_eq!(n(&format!("(len (bt_range {} %cc %mm))", t)), 4); // cc dd gg mm
    }

    #[test]
    fn wire_zigzag_and_framing() {
        assert_eq!(n("(w_zigzag [1 1])"), 1); // -1
        assert_eq!(n("(w_zigzag [0 1])"), 2); // +1
        assert_eq!(n("(tail (w_unzigzag (w_zigzag [1 5])))"), 5); // roundtrip magnitude
        // a field with a tag the reader doesn't model still round-trips by length
        let frame = "[[1 [42 0]] [[9 [7 [7 0]]] 0]]";
        assert_eq!(n(&format!("(w_unvarint (tail (head (w_decrec (w_encrec {})))))", frame)), 42);
        assert_eq!(n(&format!("(len (w_decrec (w_encrec {})))", frame)), 2);
    }

    #[test]
    fn mvcc_snapshot_isolation_and_conflict() {
        let store = "(mvcc_commit (mvcc_commit (mvcc_new 0) %x 1 5) %x 2 15)";
        assert_eq!(n(&format!("(tail (mvcc_read {} %x 10))", store)), 1); // snapshot sees v1
        assert_eq!(n(&format!("(tail (mvcc_read {} %x 20))", store)), 2); // later snapshot sees v2
        // writing x across the conflicting commit aborts; writing y is safe
        assert_eq!(run(&format!("(head (mvcc_try {} [[%x 9] 0] 10 18))", store)).unwrap(), crate::knot::cord("abort"));
        assert_eq!(run(&format!("(head (mvcc_try {} [[%y 9] 0] 10 18))", store)).unwrap(), crate::knot::cord("commit"));
    }

    #[test]
    fn mvcc_ssi_prevents_write_skew() {
        // x=y=1; T1 reads {x,y} and sets x=0 (commits ts10)
        let s0 = "(mvcc_commit (mvcc_commit (mvcc_new 0) %x 1 1) %y 1 1)";
        let after_t1 = format!("(tail (mvcc_ssi_try {} [%x [%y 0]] [[%x 0] 0] 5 10))", s0);
        // T2 also read {x,y} and now sets y=0. Disjoint write -> plain SI lets it commit (skew)...
        assert_eq!(run(&format!("(head (mvcc_try {} [[%y 0] 0] 5 15))", after_t1)).unwrap(), crate::knot::cord("commit"));
        // ...but read-set validation aborts T2 because x (which it read) changed under it.
        assert_eq!(run(&format!("(head (mvcc_ssi_try {} [%x [%y 0]] [[%y 0] 0] 5 15))", after_t1)).unwrap(), crate::knot::cord("abort"));
        // a transaction whose read set is untouched still commits under SSI
        assert_eq!(run(&format!("(head (mvcc_ssi_try {} [%x 0] [[%z 9] 0] 5 12))", s0)).unwrap(), crate::knot::cord("commit"));
    }

    #[test]
    fn hyperloglog_estimates_cardinality() {
        let ks = "(map (fn [i] -> (cat %k (numtext i))) (range 500))";
        let est = n(&format!("(hll_count (hll_addall (hll_new 8) {}))", ks));
        assert!(est > 400 && est < 600, "500 distinct should estimate within ~20%, got {}", est);
        // idempotent: re-adding the same items doesn't change the estimate
        let est2 = n(&format!("(hll_count (hll_addall (hll_addall (hll_new 8) {}) {}))", ks, ks));
        assert_eq!(est, est2);
    }

    #[test]
    fn countmin_never_undercounts() {
        let s = "(cms_addall (cms_new 4 64) [%a [%a [%a [%a [%a [%b [%b [%c 0]]]]]]]])";
        assert!(n(&format!("(cms_count {} %a)", s)) >= 5);
        assert!(n(&format!("(cms_count {} %b)", s)) >= 2);
        assert_eq!(n(&format!("(cms_count {} %z)", s)), 0); // never added -> 0
    }

    #[test]
    fn stream_tumbling_windows() {
        let ev = "[[3 5] [[7 2] [[12 8] [[15 1] [[25 4] 0]]]]]";
        // window 0 (times 3,7) sums to 7
        assert_eq!(n(&format!("(tail (head (stream_sums {} 10)))", ev)), 7);
        // two events fall before a watermark at 10
        assert_eq!(n(&format!("(stream_nlate {} 10)", ev)), 2);
    }

    #[test]
    fn raft_safety_core() {
        let log = "[[1 %a] [[1 %b] [[2 %c] 0]]]";
        // election restriction: a higher last-term beats a longer log at a lower term
        assert_eq!(n("(raft_uptodate 1 5 2 1)"), 1); // candidate NOT up-to-date
        assert_eq!(n("(raft_uptodate 2 3 2 3)"), 0); // equal -> ok
        // log matching: accept on matching prev, reject on mismatch
        assert_eq!(run(&format!("(head (raft_append {} 3 2 [[3 %d] 0]))", log)).unwrap(), crate::knot::cord("ok"));
        assert_eq!(run(&format!("(head (raft_append {} 3 9 [[3 %d] 0]))", log)).unwrap(), crate::knot::cord("reject"));
        // commit index = highest entry on a majority of 5 (matchIdx 3,3,2,1,1 -> 2)
        assert_eq!(n("(raft_commit [3 [3 [2 [1 [1 0]]]]] 5)"), 2);
    }
}
