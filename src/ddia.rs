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
    // Multi-byte printable atoms are cords (e.g. %absent). A single ASCII *letter*
    // is also shown as a cord (graph nodes %a, mvcc keys %x), but single small
    // numbers stay numbers (counts, %n indices like 1,2,3) so we don't mistake the
    // integer 97 for the letter 'a'.
    let printable = !b.is_empty() && b.iter().all(|&c| (0x20..0x7f).contains(&c));
    let single_letter = b.len() == 1 && (b[0] as char).is_ascii_alphabetic();
    if (b.len() >= 2 && printable) || single_letter {
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
        row("write skew under plain SI: T2 commits (anomaly)", &format!("(head (mvcc_try {} [[%y 0] 0] 5 15))", after_t1));
        row("write skew under serializable read-set validation: T2 aborts", &format!("(head (mvcc_ssi_try {} [%x [%y 0]] [[%y 0] 0] 5 15))", after_t1));
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
             "LSM + WAL recovery + Bloom + secondary index + MVCC snapshots + serializable txns (OCC) + leaderless replication + 2PC");
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
        // serializable transactions and the write-skew anomaly (the same doctors-
        // on-call scenario, run two ways): snapshot isolation lets both writers
        // through (anomaly), serializable read-set validation aborts the second.
        row("snapshot read ignores a later write",
            "(let d = (db_sample 0) in let s = (db_begin d) in (db_txget (db_put d %u1 [[1 %changed] 0]) s %u1))");
        row("txn commit (read set still valid)",
            "(head (db_commit (db_sample 0) (db_begin (db_sample 0)) [%u1 0] [[%u4 [[1 %dave] 0]] 0]))");
        row("write skew under SNAPSHOT ISOLATION: 2nd txn commits (anomaly)", "(db_skew_si 0)");
        row("write skew under SERIALIZABLE (OCC): 2nd txn aborts (prevented)", "(db_skew_ser 0)");
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

/// Demos for lib/algo.lat — the five algorithm-design paradigms from Skiena's
/// "The Algorithm Design Manual", the technique catalog behind most coding
/// interviews. Topics: dc (divide & conquer), dp, greedy, backtracking, graph.
pub fn cmd_algo(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("The Algorithm Design Manual (Skiena) — interview techniques in Latte (lib/algo.lat)");
    if all {
        println!("topics: dc (divide & conquer)  dp  greedy  backtracking  graph");
    }

    if all || topic == "dc" {
        head("Divide & conquer (Ch.4)",
             "split the problem, solve the parts, combine — merge sort, binary search, quickselect");
        row("merge sort an unsorted list", "(al_msort (al_nums0 0))");
        row("binary search for 7 (index)", "(al_bsearch (al_sorted0 0) 7)");
        row("binary search for 4 (absent)", "(al_bsearch (al_sorted0 0) 4)");
        row("quickselect: smallest (k=0)", "(al_qselect (al_nums0 0) 0)");
        row("quickselect: median (k=3)", "(al_qselect (al_nums0 0) 3)");
    }

    if all || topic == "dp" {
        head("Dynamic programming (Ch.8)",
             "cache sub-answers and build up — coin change, edit distance, longest increasing subsequence");
        row("min coins for 11 from {1,5,6,9}", "(al_coins (al_coins0 0) 11)");
        row("min coins for 3 from {5,6} (none)", "(al_coins [ 5 [ 6 0 ] ] 3)");
        row("edit distance kitten -> sitting",
            "(al_edit [ %k [ %i [ %t [ %t [ %e [ %n 0 ] ] ] ] ] ] [ %s [ %i [ %t [ %t [ %i [ %n [ %g 0 ] ] ] ] ] ] ])");
        row("LIS length of [3 1 4 1 5 9 2 6]",
            "(al_lis [ 3 [ 1 [ 4 [ 1 [ 5 [ 9 [ 2 [ 6 0 ] ] ] ] ] ] ] ])");
    }

    if all || topic == "greedy" {
        head("Greedy (Ch.8)",
             "take the locally-best choice and never revisit — interval scheduling (earliest finish first)");
        row("max non-overlapping intervals", "(al_sched (al_ivs0 0))");
        row("the schedule, sorted by finish", "(al_isort (al_ivs0 0))");
    }

    if all || topic == "backtracking" {
        head("Backtracking (Ch.7)",
             "enumerate the search space, prune dead ends — subsets, permutations, N-queens");
        row("all subsets of {1,2,3} (2^3=8)", "(al_subsets [ 1 [ 2 [ 3 0 ] ] ])");
        row("count permutations of {1,2,3}", "(len (al_perms [ 1 [ 2 [ 3 0 ] ] ]))");
        row("N-queens solutions, n=4", "(al_queens 4)");
        row("N-queens solutions, n=6", "(al_queens 6)");
        row("N-queens solutions, n=8", "(al_queens 8)");
    }

    if all || topic == "graph" {
        head("Graph traversal (Ch.5)",
             "explore a graph systematically — BFS shortest path, DFS order, topological sort (DAG: a->b,c; b,c->d; d->e)");
        row("BFS shortest edges a -> e", "(al_bfs (al_graph0 0) %a %e)");
        row("BFS distance to itself a -> a", "(al_bfs (al_graph0 0) %a %a)");
        row("DFS visit order from a", "(al_dfs (al_graph0 0) %a)");
        row("topological order of the DAG", "(al_topo (al_graph0 0) (al_nodes0 0))");
    }

    if all {
        println!("\nEvery arm is in lib/algo.lat (prefix al_), heavily commented by paradigm.");
        println!("See text/algorithm-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/dsa.lat — the data structures and coding PATTERNS a coding
/// interview expects (the companion to the algo.lat paradigms). Topics:
/// pointers, stack, prefix, heap, bst, trie, unionfind.
pub fn cmd_dsa(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Coding-interview data structures & patterns in Latte (lib/dsa.lat)");
    if all {
        println!("topics: pointers  stack  prefix  heap  bst  trie  unionfind");
    }

    if all || topic == "pointers" {
        head("Pointer patterns",
             "two pointers (sorted pair sum, palindrome), sliding window (max window, longest unique), fast & slow (cycle detection)");
        row("two-pointer pair sum to 9 (indices)", "(ds_pairsum (ds_arr0 0) 9)");
        row("two-pointer pair sum to 100 (none)", "(ds_pairsum (ds_arr0 0) 100)");
        row("is 'racecar' a palindrome (1=yes)",
            "(ds_ispalin [ %r [ %a [ %c [ %e [ %c [ %a [ %r 0 ] ] ] ] ] ] ])");
        row("sliding window: max sum of 2 in a row", "(ds_maxwin (ds_win0 0) 2)");
        row("longest substring w/o repeat (abcabcbb)", "(ds_longestuniq (ds_str0 0))");
        row("Floyd cycle detection: cyclic list (1)", "(ds_hascycle (ds_cyclic 0) %a)");
        row("Floyd cycle detection: acyclic list (0)", "(ds_hascycle (ds_acyclic 0) %a)");
    }

    if all || topic == "stack" {
        head("Stack",
             "balanced brackets, RPN evaluation, and a monotonic stack for next-greater-element");
        row("balanced brackets ([]{}) (1=yes)", "(ds_balanced (ds_bal0 0))");
        row("unbalanced brackets ([)] (0=no)", "(ds_balanced (ds_bal1 0))");
        row("evaluate RPN (3 4 + 5 *) = 35", "(ds_rpn (ds_rpn0 0))");
        row("next greater element of [2 1 3]", "(ds_nextgreater [ 2 [ 1 [ 3 0 ] ] ])");
    }

    if all || topic == "prefix" {
        head("Prefix sums",
             "precompute running totals once, then answer any range-sum query in O(1)");
        row("prefix table of [2 4 6]", "(ds_prefix [ 2 [ 4 [ 6 0 ] ] ])");
        row("range sum of [2 4 6] over [1,3)", "(ds_rangesum (ds_prefix [ 2 [ 4 [ 6 0 ] ] ]) 1 3)");
    }

    if all || topic == "heap" {
        head("Heap / priority queue (leftist heap)",
             "a purely-functional min-heap; heapsort and top-K both fall out of repeated delete-min");
        row("heapsort an unsorted list", "(ds_heapsort (ds_unsort0 0))");
        row("the 3 smallest elements", "(ds_topk (ds_unsort0 0) 3)");
    }

    if all || topic == "bst" {
        head("Binary search tree",
             "ordered insert/lookup in O(height); inorder traversal yields sorted keys");
        row("inorder of a BST built from 5,2,8,1,9",
            "(ds_bstinorder (ds_bstins (ds_bstins (ds_bstins (ds_bstins (ds_bstins 0 5) 2) 8) 1) 9))");
        row("contains 8 (0=yes)", "(ds_bsthas (ds_bstins (ds_bstins 0 5) 8) 8)");
        row("contains 7 (1=no)", "(ds_bsthas (ds_bstins (ds_bstins 0 5) 8) 7)");
    }

    if all || topic == "trie" {
        head("Trie (prefix tree)",
             "store words by character path; membership and prefix tests cost O(length of query)");
        row("insert cat,car; has 'cat' (0=yes)",
            "(ds_thas (ds_tins (ds_tins (ds_tnew 0) [ %c [ %a [ %t 0 ] ] ]) [ %c [ %a [ %r 0 ] ] ]) [ %c [ %a [ %t 0 ] ] ])");
        row("has 'ca' as a word (1=no)",
            "(ds_thas (ds_tins (ds_tnew 0) [ %c [ %a [ %t 0 ] ] ]) [ %c [ %a 0 ] ])");
        row("'ca' as a prefix (0=yes)",
            "(ds_tprefix (ds_tins (ds_tnew 0) [ %c [ %a [ %t 0 ] ] ]) [ %c [ %a 0 ] ])");
    }

    if all || topic == "unionfind" {
        head("Union-Find (disjoint set)",
             "near-O(1) connectivity: find a representative root, union two roots, count components");
        row("components after union(a,b),(c,d) of 4 (2)",
            "(ds_ufcount (ds_ufunion (ds_ufunion (ds_ufnew [ %a [ %b [ %c [ %d 0 ] ] ] ]) %a %b) %c %d))");
        row("a connected to b after union (0=yes)",
            "(ds_ufconnected (ds_ufunion (ds_ufnew [ %a [ %b [ %c 0 ] ] ]) %a %b) %a %b)");
        row("a connected to c (1=no)",
            "(ds_ufconnected (ds_ufunion (ds_ufnew [ %a [ %b [ %c 0 ] ] ]) %a %b) %a %c)");
    }

    if all {
        println!("\nEvery arm is in lib/dsa.lat (prefix ds_), with the recognition cue per section.");
        println!("See text/dsa-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/wgraph.lat — weighted graph algorithms (the tier above BFS/DFS):
/// Dijkstra shortest paths, and Kruskal + Prim minimum spanning trees. Topics:
/// dijkstra, mst.
pub fn cmd_wgraph(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Weighted graph algorithms in Latte (lib/wgraph.lat)");
    println!("  sample graph (undirected): a-b 4, a-c 1, b-c 2, b-d 5, c-d 8, d-e 3");
    if all {
        println!("topics: dijkstra  mst");
    }

    if all || topic == "dijkstra" {
        head("Dijkstra — single-source shortest paths",
             "settle the closest unsettled node, relax its edges; non-negative weights, O(V^2) here");
        row("shortest distances from a (whole map)", "(wg_dijkstra (wg_graph0 0) (wg_nodes0 0) %a)");
        row("shortest distance a -> e", "(wg_distto (wg_graph0 0) (wg_nodes0 0) %a %e)");
        row("shortest distance a -> b (via c!)", "(wg_distto (wg_graph0 0) (wg_nodes0 0) %a %b)");
    }

    if all || topic == "mst" {
        head("Minimum spanning tree (Kruskal & Prim)",
             "cheapest set of edges connecting every node; Kruskal sorts edges + union-find, Prim grows one tree");
        row("Kruskal MST total weight", "(wg_kruskal (wg_nodes0 0) (wg_edges0 0))");
        row("Prim MST total weight (agrees)", "(wg_prim (wg_graph0 0) (wg_nodes0 0))");
        row("edges sorted by weight (Kruskal order)", "(wg_esort (wg_edges0 0))");
    }

    if all {
        println!("\nEvery arm is in lib/wgraph.lat (prefix wg_); Kruskal reuses union-find from dsa.lat.");
        println!("See text/weighted-graphs.md for the interactive tutorial.");
    }
}

/// Demos for lib/numth.lat — number theory & math warm-ups: GCD/LCM, primality and
/// the Sieve of Eratosthenes, modular exponentiation, combinatorics, digit ops.
/// Topics: divisibility, primes, modular, combinatorics, digits.
pub fn cmd_numth(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Number theory & math techniques in Latte (lib/numth.lat)");
    if all {
        println!("topics: divisibility  primes  modular  combinatorics  digits");
    }

    if all || topic == "divisibility" {
        head("Divisibility — Euclid's algorithm",
             "gcd(a,b) = gcd(b, a mod b); lcm built from it");
        row("gcd(48, 36)", "(nt_gcd 48 36)");
        row("gcd(17, 5) (coprime -> 1)", "(nt_gcd 17 5)");
        row("lcm(4, 6)", "(nt_lcm 4 6)");
    }

    if all || topic == "primes" {
        head("Primes",
             "trial-division primality (0 = prime), and the Sieve of Eratosthenes");
        row("is 7 prime (0=yes)", "(nt_isprime 7)");
        row("is 9 prime (1=no)", "(nt_isprime 9)");
        row("primes up to 30 (sieve)", "(nt_sieve 30)");
    }

    if all || topic == "modular" {
        head("Modular arithmetic",
             "modular exponentiation by squaring — base^exp mod m in O(log exp), the RSA workhorse");
        row("2^10 mod 1000", "(nt_modpow 2 10 1000)");
        row("3^5 mod 7", "(nt_modpow 3 5 7)");
        row("7^256 mod 13 (stays small)", "(nt_modpow 7 256 13)");
    }

    if all || topic == "combinatorics" {
        head("Combinatorics",
             "factorial, binomial coefficient (multiplicative, no giant factorials), Fibonacci");
        row("5!", "(nt_fact 5)");
        row("C(10, 3) ways to choose", "(nt_choose 10 3)");
        row("Fibonacci F(10)", "(nt_fib 10)");
    }

    if all || topic == "digits" {
        head("Digit operations",
             "peel decimal digits with mod 10 / div 10");
        row("digit sum of 1234", "(nt_digitsum 1234)");
        row("reverse digits of 1234", "(nt_reverse 1234)");
    }

    if all {
        println!("\nEvery arm is in lib/numth.lat (prefix nt_), using only integer arithmetic.");
        println!("See text/number-theory.md for the interactive tutorial.");
    }
}

/// Demos for lib/bits.lat — bit-manipulation techniques on the bitwise vocabulary
/// added to std (band/bor/bxor/shl/shr/bit/popcount). Topics: xor, tests,
/// enumeration, counting.
pub fn cmd_bits(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Bit manipulation in Latte (lib/bits.lat, on std bitwise ops)");
    if all {
        println!("topics: xor  tests  enumeration  counting");
    }

    if all || topic == "xor" {
        head("XOR tricks",
             "self-cancellation (x^x=0) does the bookkeeping: single number, missing number, add without '+'");
        row("single number in [4 1 2 1 2]", "(bm_single (bm_dup0 0))");
        row("missing number in 0..3 = [0 1 3]", "(bm_missing (bm_gap0 0) 3)");
        row("add 5 + 3 using only XOR/AND/shift", "(bm_add 5 3)");
        row("add 123 + 456 the same way", "(bm_add 123 456)");
    }

    if all || topic == "tests" {
        head("Bit tests & single-bit writes",
             "power-of-two check, and get/set/clear/toggle one bit");
        row("is 16 a power of two (1=yes)", "(bm_ispow2 16)");
        row("is 12 a power of two (0=no)", "(bm_ispow2 12)");
        row("set bit 3 of 0 -> 8", "(bm_set 0 3)");
        row("clear bit 1 of 15 -> 13", "(bm_clear 15 1)");
        row("clear lowest set bit of 12 -> 8", "(bm_clearlow 12)");
    }

    if all || topic == "enumeration" {
        head("Subset enumeration by bitmask",
             "each integer in [0, 2^n) is a subset; bit j says 'include element j' — the basis of bitmask DP");
        row("all subsets of {3,1,2} (2^3 = 8)", "(bm_subsets (bm_set0 0))");
        row("how many subsets", "(len (bm_subsets (bm_set0 0)))");
    }

    if all || topic == "counting" {
        head("Counting bits",
             "Hamming distance = popcount of XOR; counting-bits DP uses popcount(i)=popcount(i>>1)+(i&1)");
        row("Hamming distance of 4 and 1", "(bm_hamming 4 1)");
        row("popcounts of 0..7 (DP)", "(bm_countbits 7)");
    }

    if all {
        println!("\nEvery arm is in lib/bits.lat (prefix bm_), on the std bitwise ops (band/bor/bxor/shl/shr).");
        println!("See text/bit-manipulation.md for the interactive tutorial.");
    }
}

/// Demos for lib/strings.lat — string-algorithm techniques on the cord<->byte-list
/// helpers in std. Topics: analysis, anagrams, search, families.
pub fn cmd_strings(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("String algorithms in Latte (lib/strings.lat)");
    if all {
        println!("topics: analysis  anagrams  search  families");
    }

    if all || topic == "analysis" {
        head("Analysis",
             "reverse, palindrome test, character count (a char is a one-byte cord)");
        row("reverse %hello", "(st_rev %hello)");
        row("is %racecar a palindrome (0=yes)", "(st_ispalin (st_word0 0))");
        row("is %hello a palindrome (1=no)", "(st_ispalin (st_word1 0))");
        row("count 'l' in %hello", "(st_count %hello %l)");
    }

    if all || topic == "anagrams" {
        head("Anagrams",
             "permutation test via sorted-character signature, and first non-repeating char");
        row("are %listen and %silent anagrams (0=yes)", "(st_anagram (st_anaA 0) (st_anaB 0))");
        row("are %hi and %bye anagrams (1=no)", "(st_anagram %hi %bye)");
        row("first unique char of %hello", "(st_firstuniq %hello)");
    }

    if all || topic == "search" {
        head("Substring search",
             "naive sliding window, and Rabin-Karp rolling hash (index, or text length if absent)");
        row("find %cad in %abracadabra", "(st_find (st_text0 0) %cad)");
        row("Rabin-Karp %bra in %abracadabra", "(st_rabinkarp (st_text0 0) %bra)");
        row("Rabin-Karp %xyz (absent -> 11)", "(st_rabinkarp (st_text0 0) %xyz)");
    }

    if all || topic == "families" {
        head("Families of strings",
             "longest common prefix, and run-length encoding");
        row("LCP of [%flower %flow %flight]", "(st_lcp (st_list0 0))");
        row("run-length encode %aaabbc", "(st_rle (st_run0 0))");
    }

    if all {
        println!("\nEvery arm is in lib/strings.lat (prefix st_), on the std cord helpers (bytes/frombytes/sort).");
        println!("See text/string-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/grid.lat — matrix/grid techniques: connectivity (islands/flood fill),
/// grid DP (unique paths), and transforms (transpose/rotate/spiral).
pub fn cmd_grid(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Matrix/grid algorithms in Latte (lib/grid.lat)");
    if all {
        println!("topics: connectivity  dp  transforms");
    }

    if all || topic == "connectivity" {
        head("Connectivity — flood fill",
             "number of 4-connected islands of land(1) cells; the sample grid has three");
        row("number of islands in the sample grid", "(gr_islands (gr_grid0 0))");
        row("islands in a single land cell", "(gr_islands [ [1 0] 0 ])");
        row("islands in all-water 2x2", "(gr_islands [ [0 [0 0]] [ [0 [0 0]] 0 ] ])");
    }

    if all || topic == "dp" {
        head("Grid dynamic programming",
             "count distinct right/down paths from top-left to bottom-right (rolling-array DP)");
        row("unique paths in a 3x3 grid", "(gr_pathcount 3 3)");
        row("unique paths in a 3x7 grid", "(gr_pathcount 3 7)");
    }

    if all || topic == "transforms" {
        head("Transforms",
             "transpose, rotate 90 clockwise, and spiral-order traversal of 1..9");
        row("transpose the 3x3 matrix", "(gr_transpose (gr_mat0 0))");
        row("rotate it 90 clockwise", "(gr_rotate (gr_mat0 0))");
        row("spiral-order traversal", "(gr_spiral (gr_mat0 0))");
    }

    if all {
        println!("\nEvery arm is in lib/grid.lat (prefix gr_); a grid is a list of rows.");
        println!("See text/grid-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/design.lat — data-structure design classics: min-stack, queue from
/// two stacks, and an LRU cache. Topics: minstack, queue, lru.
pub fn cmd_design(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Data-structure design in Latte (lib/design.lat)");
    if all {
        println!("topics: minstack  queue  lru");
    }

    if all || topic == "minstack" {
        head("Min-stack — getMin in O(1)",
             "each node stores the minimum as of its own push, so the top always knows the current min");
        row("push 5,2,7,1 then top", "(dz_mstop (dz_ms0 0))");
        row("current minimum (O(1))", "(dz_msmin (dz_ms0 0))");
        row("minimum after one pop (back to 2)", "(dz_msmin (dz_mspop (dz_ms0 0)))");
    }

    if all || topic == "queue" {
        head("Queue from two stacks — amortised O(1) FIFO",
             "the in-stack receives, the out-stack releases; flip in->out when out runs dry");
        row("enqueue 1,2,3 then peek front", "(dz_qpeek (dz_q0 0))");
        row("dequeue once (returns 1)", "(head (dz_deq (dz_q0 0)))");
        row("dequeue twice (returns 2)", "(head (dz_deq (tail (dz_deq (dz_q0 0)))))");
    }

    if all || topic == "lru" {
        head("LRU cache — evict the least-recently-used",
             "entries kept in recency order; a put past capacity drops the back, a touch moves to front");
        row("put a,b,c into a cap-2 cache; keys", "(dz_lrukeys (dz_lru0 0))");
        row("get a (evicted -> 0)", "(dz_lruget (dz_lru0 0) %a)");
        row("touch b, then put d; keys (c gone)", "(dz_lrukeys (dz_lruput (dz_lrutouch (dz_lru0 0) %b) %d 4))");
    }

    if all {
        println!("\nEvery arm is in lib/design.lat (prefix dz_); operations return the new structure.");
        println!("See text/design-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/trees.lat — binary tree techniques: traversals, shape, paths, queries.
/// Topics: traversals, shape, paths, queries.
pub fn cmd_trees(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Binary tree algorithms in Latte (lib/trees.lat)");
    if all {
        println!("topics: traversals  shape  paths  queries");
        println!("sample tree:  5 / (3 / 2,4) / (8 / 7,9)");
    }

    if all || topic == "traversals" {
        head("Traversals",
             "the three depth-first orders differ only in where the root is visited; level-order is breadth-first");
        row("in-order (sorted for a BST)", "(tr_inorder (tr_tree0 0))");
        row("pre-order (root first)", "(tr_preorder (tr_tree0 0))");
        row("post-order (root last)", "(tr_postorder (tr_tree0 0))");
        row("level-order (breadth-first)", "(tr_levelorder (tr_tree0 0))");
    }

    if all || topic == "shape" {
        head("Shape",
             "height, node count, and invert/mirror — all one recursive template");
        row("height (max depth)", "(tr_height (tr_tree0 0))");
        row("node count", "(tr_count (tr_tree0 0))");
        row("invert, then read in-order (reversed)", "(tr_inorder (tr_invert (tr_tree0 0)))");
    }

    if all || topic == "paths" {
        head("Paths",
             "maximum sum along any root-to-leaf path");
        row("max root-to-leaf sum (5+8+9)", "(tr_maxpath (tr_tree0 0))");
    }

    if all || topic == "queries" {
        head("Queries",
             "lowest common ancestor, and validate-BST via the in-order = sorted property");
        row("LCA of 2 and 4", "(tr_lca (tr_tree0 0) 2 4)");
        row("LCA of 2 and 7 (the root)", "(tr_lca (tr_tree0 0) 2 7)");
        row("is the sample a BST (0=yes)", "(tr_isbst (tr_tree0 0))");
        row("is the broken tree a BST (1=no)", "(tr_isbst (tr_bad0 0))");
    }

    if all {
        println!("\nEvery arm is in lib/trees.lat (prefix tr_); a node is [value [left right]], empty is 0.");
        println!("See text/tree-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/dp.lat — dynamic-programming classics. Topics: oned, feasibility,
/// counting, optimisation.
pub fn cmd_dp(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Dynamic programming in Latte (lib/dp.lat)");
    if all {
        println!("topics: oned  feasibility  counting  optimisation");
    }

    if all || topic == "oned" {
        head("1-D DP — one rolling dimension",
             "climbing stairs (Fibonacci recurrence), house robber (no two adjacent)");
        row("ways to climb 5 stairs (1 or 2 at a time)", "(dp_stairs 5)");
        row("max non-adjacent sum of [2 7 9 3 1]", "(dp_rob (dp_houses0 0))");
    }

    if all || topic == "feasibility" {
        head("Feasibility — reach a target exactly",
             "subset-sum tracks reachable totals; partition reduces to it (0 = yes)");
        row("subset of {3,34,4,12,5,2} summing to 9?", "(dp_subsetsum (dp_set0 0) 9)");
        row("can {1,5,11,5} split into equal halves?", "(dp_canpartition (dp_part0 0))");
    }

    if all || topic == "counting" {
        head("Counting — how many ways",
             "coin-change counts combinations (coins looped outside, so order is ignored)");
        row("ways to make 5 from coins {1,2,5}", "(dp_coinways (dp_coins0 0) 5)");
        row("ways to make 0 (the empty way)", "(dp_coinways (dp_coins0 0) 0)");
    }

    if all || topic == "optimisation" {
        head("Optimisation — best value under a constraint",
             "0/1 knapsack (each item once) and longest common subsequence");
        row("knapsack of [w v] items, capacity 5", "(dp_knapsack (dp_items0 0) 5)");
        row("LCS length of abcde and ace", "(dp_lcs %abcde %ace)");
        row("LCS length of AGGTAB and GXTXAYB", "(dp_lcs %AGGTAB %GXTXAYB)");
    }

    if all {
        println!("\nEvery arm is in lib/dp.lat (prefix dp_): a recurrence over smaller sub-answers, each computed once.");
        println!("See text/dp-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/intervals.lat — interval techniques: merging, scheduling, set ops.
/// Topics: merging, scheduling, setops.
pub fn cmd_intervals(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Interval algorithms in Latte (lib/intervals.lat)");
    if all {
        println!("topics: merging  scheduling  setops");
        println!("(an interval is the pair [start end]; most arms sort by start first)");
    }

    if all || topic == "merging" {
        head("Merging",
             "merge overlapping intervals, and insert a new interval into a sorted disjoint list");
        row("merge [1-3][2-6][8-10][15-18]", "(iv_merge (iv_sample0 0))");
        row("insert [4 8] into [1-2][3-5][6-7][8-10][12-16]",
            "(iv_insert [ [1 2] [ [3 5] [ [6 7] [ [8 10] [ [12 16] 0 ] ] ] ] ] [4 8])");
    }

    if all || topic == "scheduling" {
        head("Scheduling — the sweep line",
             "can one person attend all? (0=yes); minimum rooms = peak simultaneous meetings");
        row("can attend all of [0-30][5-10][15-20] (1=no)", "(iv_canattend (iv_meetings0 0))");
        row("minimum rooms for those meetings", "(iv_minrooms (iv_meetings0 0))");
        row("minimum rooms for [1-10][2-10][3-10]", "(iv_minrooms [ [1 10] [ [2 10] [ [3 10] 0 ] ] ])");
    }

    if all || topic == "setops" {
        head("Set operations",
             "intersection of two sorted interval lists (two-pointer); total covered length");
        row("intersection of the two sample lists", "(iv_intersect (iv_a0 0) (iv_b0 0))");
        row("total length covered by the sample set", "(iv_coverage (iv_sample0 0))");
    }

    if all {
        println!("\nEvery arm is in lib/intervals.lat (prefix iv_); sorting by start turns overlaps into one sweep.");
        println!("See text/interval-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/search.lat — binary search variants. Topics: boundaries, rotated,
/// slope, answer.
pub fn cmd_search(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Binary search techniques in Latte (lib/search.lat)");
    if all {
        println!("topics: boundaries  rotated  slope  answer");
    }

    if all || topic == "boundaries" {
        head("Boundaries in a sorted array",
             "lower/upper bound locate where a value's run begins and ends (sample: [1 2 2 2 3 4])");
        row("lower bound of 2 (first index)", "(se_lowerbound (se_sorted0 0) 2)");
        row("upper bound of 2 (one past last)", "(se_upperbound (se_sorted0 0) 2)");
        row("occurrence count of 2", "(se_count (se_sorted0 0) 2)");
        row("does 3 occur (0=yes)", "(se_contains (se_sorted0 0) 3)");
    }

    if all || topic == "rotated" {
        head("Rotated sorted array",
             "one half is always sorted; sample is [4 5 6 7 0 1 2]");
        row("find 0 (index 4)", "(se_rotated (se_rot0 0) 0)");
        row("find 9 (absent -> length 7)", "(se_rotated (se_rot0 0) 9)");
        row("minimum (the rotation point)", "(se_rotmin (se_rot0 0))");
    }

    if all || topic == "slope" {
        head("Find a peak — binary search without sorting",
             "follow the uphill slope; sample [1 2 3 1] peaks at index 2");
        row("a peak index of [1 2 3 1]", "(se_peak (se_peak0 0))");
    }

    if all || topic == "answer" {
        head("Binary search ON THE ANSWER",
             "when 'is value k good?' is monotone, search the value itself — no array needed");
        row("integer sqrt of 17 (largest k, k*k<=17)", "(se_isqrt 17)");
        row("Koko min eating speed, piles [3 6 7 11], 8 hours", "(se_minspeed (se_piles0 0) 8)");
    }

    if all {
        println!("\nEvery arm is in lib/search.lat (prefix se_): keep a range containing the boundary, halve it each step.");
        println!("See text/search-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/graphs.lat — graph decision problems. Topics: ordering, components,
/// bipartite.
pub fn cmd_graphs(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Graph algorithms in Latte (lib/graphs.lat)");
    if all {
        println!("topics: ordering  components  bipartite");
        println!("(a graph is an assoc list node -> out-neighbours; undirected samples list edges both ways)");
    }

    if all || topic == "ordering" {
        head("Ordering & cycles — directed graphs",
             "Kahn topological sort; is-DAG decides course-schedule feasibility");
        row("topological order of the DAG sample", "(gp_toposort (gp_dag0 0))");
        row("is the DAG sample acyclic (0=yes)", "(gp_isdag (gp_dag0 0))");
        row("is the 3-cycle 0->1->2->0 acyclic (1=no)", "(gp_isdag (gp_cyc0 0))");
    }

    if all || topic == "components" {
        head("Components & reachability — undirected graphs",
             "flood fill counts connected pieces and answers path queries");
        row("connected components of {0,1,2}{3,4}{5}", "(gp_components (gp_undir0 0))");
        row("path from 0 to 2 (same piece, 0=yes)", "(gp_reach (gp_undir0 0) 0 2)");
        row("path from 0 to 3 (different piece, 1=no)", "(gp_reach (gp_undir0 0) 0 3)");
    }

    if all || topic == "bipartite" {
        head("Bipartite test — 2-colouring",
             "even cycles 2-colour cleanly; an odd cycle forces a clash");
        row("is the 4-cycle 0-1-2-3-0 bipartite (0=yes)", "(gp_bipartite (gp_bip0 0))");
        row("is the triangle 0-1-2-0 bipartite (1=no)", "(gp_bipartite (gp_odd0 0))");
    }

    if all {
        println!("\nEvery arm is in lib/graphs.lat (prefix gp_): adjacency assoc list, traversal by flood fill or in-degree.");
        println!("See text/graph-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/backtrack.lat — backtracking / combinatorial search. Topics: parens,
/// combos, partition, grid.
pub fn cmd_backtrack(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Backtracking in Latte (lib/backtrack.lat)");
    if all {
        println!("topics: parens  combos  partition  grid");
        println!("(string-valued results are reported as COUNTS, since multi-byte cords show as integers)");
    }

    if all || topic == "parens" {
        head("Build a string — generate valid parentheses",
             "constraint pruning: only open while opens remain, only close while owed (counts are Catalan numbers)");
        row("number of valid strings with 2 pairs", "(len (bk_parens 2))");
        row("number with 3 pairs", "(len (bk_parens 3))");
        row("number with 4 pairs", "(len (bk_parens 4))");
    }

    if all || topic == "combos" {
        head("Pick — combination sum and k-combinations",
             "candidate sets are kept non-decreasing to avoid duplicate orderings");
        row("combination sum of {2,3,6,7} to 7", "(bk_combsum (bk_cands0 0) 7)");
        row("all 2-combinations of {1..4}", "(bk_combine 4 2)");
        row("count of 3-combinations of {1..5}", "(len (bk_combine 5 3))");
    }

    if all || topic == "partition" {
        head("Split a string — palindrome partitioning",
             "for each palindromic prefix, recurse on the rest");
        row("number of palindrome partitions of 'aab'", "(len (bk_partition %aab))");
        row("number for 'aaa'", "(len (bk_partition %aaa))");
        row("is 'racecar' a palindrome (0=yes)", "(bk_ispal (bytes %racecar))");
    }

    if all || topic == "grid" {
        head("Grid path — word search",
             "DFS from each cell, marking used cells; board is ABCE / SFCS / ADEE");
        row("does 'ABCCED' appear (0=yes)", "(bk_wordsearch (bk_grid0 0) %ABCCED)");
        row("does 'SEE' appear (0=yes)", "(bk_wordsearch (bk_grid0 0) %SEE)");
        row("does 'ABCB' appear (1=no, reuse forbidden)", "(bk_wordsearch (bk_grid0 0) %ABCB)");
    }

    if all {
        println!("\nEvery arm is in lib/backtrack.lat (prefix bk_): each choice branches; a step's answer is its branches concatenated.");
        println!("See text/backtrack-techniques.md for the interactive tutorial.");
    }
}

/// Demos for lib/greedy.lat — greedy algorithms. Topics: jump, gas, scan, schedule.
pub fn cmd_greedy(args: &[String]) {
    let topic = args.first().map(|s| s.as_str()).unwrap_or("all");
    let all = topic == "all";
    println!("Greedy algorithms in Latte (lib/greedy.lat)");
    if all {
        println!("topics: jump  gas  scan  schedule");
    }

    if all || topic == "jump" {
        head("Reachability — the jump game",
             "track the furthest reach; sample [2 3 1 1 4] is reachable in 2 jumps");
        row("can [2 3 1 1 4] reach the end (0=yes)", "(gd_canjump (gd_jump0 0))");
        row("can [3 2 1 0 4] reach the end (1=no)", "(gd_canjump (gd_jump1 0))");
        row("minimum jumps for [2 3 1 1 4]", "(gd_minjumps (gd_jump0 0))");
    }

    if all || topic == "gas" {
        head("Circuit — gas station",
             "feasible iff total gas >= total cost; start right after the lowest tank dip");
        row("starting station for the loop", "(gd_gas (gd_gasG0 0) (gd_gasC0 0))");
        row("impossible case returns n (=3)", "(gd_gas [2 [3 [4 0]]] [3 [4 [3 0]]])");
    }

    if all || topic == "scan" {
        head("Scanning & two-pass",
             "partition labels (greedy boundaries) and candy (max of two one-way passes)");
        row("partition-label sizes of 'abac'", "(gd_partition %abac)");
        row("partition the classic LeetCode string", "(gd_partition %ababcbacadefegdehijhklij)");
        row("candy for ratings [1 0 2]", "(gd_candy (gd_rat0 0))");
    }

    if all || topic == "schedule" {
        head("Counting — task scheduler",
             "the most frequent task paces the timeline; idles only when tasks are too few");
        row("two tasks x3, cooldown 2", "(gd_taskschedule (gd_freqs0 0) 2)");
        row("two tasks x3, no cooldown", "(gd_taskschedule (gd_freqs0 0) 0)");
    }

    if all {
        println!("\nEvery arm is in lib/greedy.lat (prefix gd_): one local choice per step, justified by an exchange argument.");
        println!("See text/greedy-techniques.md for the interactive tutorial.");
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    fn n(expr: &str) -> u128 {
        run(expr).unwrap().as_atom().unwrap().to_u128().unwrap()
    }

    #[test]
    fn algo_divide_and_conquer() {
        // merge sort orders the list
        assert_eq!(run("(al_msort (al_nums0 0))").unwrap(), run("(al_sorted0 0)").unwrap());
        // binary search finds present keys and reports %nf for absent ones
        assert_eq!(n("(al_bsearch (al_sorted0 0) 7)"), 4);
        assert_eq!(run("(al_bsearch (al_sorted0 0) 4)").unwrap(), crate::knot::cord("nf"));
        // quickselect returns order statistics without sorting
        assert_eq!(n("(al_qselect (al_nums0 0) 0)"), 1); // min
        assert_eq!(n("(al_qselect (al_nums0 0) 3)"), 5); // median of 7 values
        assert_eq!(n("(al_qselect (al_nums0 0) 6)"), 9); // max
    }

    #[test]
    fn algo_dynamic_programming() {
        // coin change: 11 = 5 + 6 -> 2 coins; 3 from {5,6} is impossible
        assert_eq!(n("(al_coins (al_coins0 0) 11)"), 2);
        assert_eq!(run("(al_coins [ 5 [ 6 0 ] ] 3)").unwrap(), crate::knot::cord("nf"));
        // edit distance kitten -> sitting is the classic 3 (Skiena's example)
        assert_eq!(n("(al_edit [ %k [ %i [ %t [ %t [ %e [ %n 0 ] ] ] ] ] ] \
                                [ %s [ %i [ %t [ %t [ %i [ %n [ %g 0 ] ] ] ] ] ] ])"), 3);
        // longest increasing subsequence of [3 1 4 1 5 9 2 6] is length 4
        assert_eq!(n("(al_lis [ 3 [ 1 [ 4 [ 1 [ 5 [ 9 [ 2 [ 6 0 ] ] ] ] ] ] ] ])"), 4);
    }

    #[test]
    fn algo_greedy_interval_scheduling() {
        // the sample intervals admit at most 2 mutually non-overlapping ones
        assert_eq!(n("(al_sched (al_ivs0 0))"), 2);
        // a trivially non-overlapping set keeps all of them
        assert_eq!(n("(al_sched [ [1 2] [ [3 4] [ [5 6] 0 ] ] ])"), 3);
    }

    #[test]
    fn algo_backtracking() {
        // 2^n subsets, n! permutations
        assert_eq!(n("(len (al_subsets [ 1 [ 2 [ 3 0 ] ] ]))"), 8);
        assert_eq!(n("(len (al_perms [ 1 [ 2 [ 3 0 ] ] ]))"), 6);
        // N-queens solution counts (the canonical sequence)
        assert_eq!(n("(al_queens 4)"), 2);
        assert_eq!(n("(al_queens 6)"), 4);
        assert_eq!(n("(al_queens 8)"), 92);
    }

    #[test]
    fn algo_graph_traversal() {
        // shortest path a->e is 3 edges (a->b->d->e); a->a is 0
        assert_eq!(n("(al_bfs (al_graph0 0) %a %e)"), 3);
        assert_eq!(n("(al_bfs (al_graph0 0) %a %a)"), 0);
        // an unreachable target reports %nf
        assert_eq!(run("(al_bfs [ [ %x [ %y 0 ] ] [ [ %y 0 ] [ [ %z 0 ] 0 ] ] ] %x %z)").unwrap(),
                   crate::knot::cord("nf"));
        // DFS visits every node; topological order respects every edge
        assert_eq!(n("(len (al_dfs (al_graph0 0) %a))"), 5);
        assert_eq!(n("(len (al_topo (al_graph0 0) (al_nodes0 0)))"), 5);
        // in the topo order, a (a source) comes before e (the sink)
        assert_eq!(run("(head (al_topo (al_graph0 0) (al_nodes0 0)))").unwrap(),
                   crate::knot::cord("a"));
        assert_eq!(run("(last (al_topo (al_graph0 0) (al_nodes0 0)))").unwrap(),
                   crate::knot::cord("e"));
    }

    #[test]
    fn dsa_pointer_patterns() {
        // two pointers on a sorted array: indices of a pair summing to target, else %nf
        assert_eq!(run("(ds_pairsum (ds_arr0 0) 9)").unwrap(), run("[0 [1 0]]").unwrap());
        assert_eq!(run("(ds_pairsum (ds_arr0 0) 26)").unwrap(), run("[2 [3 0]]").unwrap());
        assert_eq!(run("(ds_pairsum (ds_arr0 0) 100)").unwrap(), crate::knot::cord("nf"));
        // palindrome by two pointers
        assert_eq!(n("(ds_ispalin [ %r [ %a [ %c [ %e [ %c [ %a [ %r 0 ] ] ] ] ] ] ])"), 1);
        assert_eq!(n("(ds_ispalin [ %a [ %b [ %c 0 ] ] ])"), 0);
        // sliding windows
        assert_eq!(n("(ds_maxwin (ds_win0 0) 2)"), 6);
        assert_eq!(n("(ds_longestuniq (ds_str0 0))"), 3); // abcabcbb -> 3
        // Floyd cycle detection
        assert_eq!(n("(ds_hascycle (ds_cyclic 0) %a)"), 1);
        assert_eq!(n("(ds_hascycle (ds_acyclic 0) %a)"), 0);
    }

    #[test]
    fn dsa_stack() {
        assert_eq!(n("(ds_balanced (ds_bal0 0))"), 1); // ([]{})
        assert_eq!(n("(ds_balanced (ds_bal1 0))"), 0); // ([)]
        assert_eq!(n("(ds_rpn (ds_rpn0 0))"), 35);     // (3+4)*5
        // next greater element: 2->3, 1->3, 3->none
        assert_eq!(run("(ds_nextgreater [ 2 [ 1 [ 3 0 ] ] ])").unwrap(),
                   run("[3 [3 [%none 0]]]").unwrap());
    }

    #[test]
    fn dsa_prefix_sums() {
        assert_eq!(run("(ds_prefix [ 2 [ 4 [ 6 0 ] ] ])").unwrap(), run("[0 [2 [6 [12 0]]]]").unwrap());
        assert_eq!(n("(ds_rangesum (ds_prefix [ 2 [ 4 [ 6 0 ] ] ]) 1 3)"), 10); // 4+6
        assert_eq!(n("(ds_rangesum (ds_prefix [ 2 [ 4 [ 6 0 ] ] ]) 0 3)"), 12); // whole array
    }

    #[test]
    fn dsa_heap() {
        // heapsort orders the list; top-k returns the k smallest in order
        assert_eq!(run("(ds_heapsort (ds_unsort0 0))").unwrap(), run("[1 [2 [3 [5 [8 [9 0]]]]]]").unwrap());
        assert_eq!(run("(ds_topk (ds_unsort0 0) 3)").unwrap(), run("[1 [2 [3 0]]]").unwrap());
    }

    #[test]
    fn dsa_bst() {
        let t = "(ds_bstins (ds_bstins (ds_bstins (ds_bstins (ds_bstins 0 5) 2) 8) 1) 9)";
        // inorder traversal of a BST is sorted
        assert_eq!(run(&format!("(ds_bstinorder {})", t)).unwrap(), run("[1 [2 [5 [8 [9 0]]]]]").unwrap());
        assert_eq!(n(&format!("(ds_bsthas {} 8)", t)), 0); // present
        assert_eq!(n(&format!("(ds_bsthas {} 7)", t)), 1); // absent
    }

    #[test]
    fn dsa_trie() {
        let t = "(ds_tins (ds_tins (ds_tnew 0) [ %c [ %a [ %t 0 ] ] ]) [ %c [ %a [ %r 0 ] ] ])";
        assert_eq!(n(&format!("(ds_thas {} [ %c [ %a [ %t 0 ] ] ])", t)), 0);    // 'cat' is a word
        assert_eq!(n(&format!("(ds_thas {} [ %c [ %a 0 ] ])", t)), 1);           // 'ca' is not a word
        assert_eq!(n(&format!("(ds_tprefix {} [ %c [ %a 0 ] ])", t)), 0);        // 'ca' is a prefix
        assert_eq!(n(&format!("(ds_tprefix {} [ %d [ %o 0 ] ])", t)), 1);        // 'do' is not
    }

    #[test]
    fn dsa_union_find() {
        let uf4 = "(ds_ufunion (ds_ufunion (ds_ufnew [ %a [ %b [ %c [ %d 0 ] ] ] ]) %a %b) %c %d)";
        assert_eq!(n(&format!("(ds_ufcount {})", uf4)), 2); // {a,b} and {c,d}
        let uf3 = "(ds_ufunion (ds_ufnew [ %a [ %b [ %c 0 ] ] ]) %a %b)";
        assert_eq!(n(&format!("(ds_ufconnected {} %a %b)", uf3)), 0); // same set
        assert_eq!(n(&format!("(ds_ufconnected {} %a %c)", uf3)), 1); // different sets
    }

    #[test]
    fn wgraph_dijkstra() {
        // shortest distances from a in the sample graph: a0 b3 c1 d8 e11
        assert_eq!(n("(wg_distto (wg_graph0 0) (wg_nodes0 0) %a %c)"), 1);
        assert_eq!(n("(wg_distto (wg_graph0 0) (wg_nodes0 0) %a %b)"), 3); // a-c-b beats a-b
        assert_eq!(n("(wg_distto (wg_graph0 0) (wg_nodes0 0) %a %d)"), 8);
        assert_eq!(n("(wg_distto (wg_graph0 0) (wg_nodes0 0) %a %e)"), 11);
        // source to itself is 0
        assert_eq!(n("(wg_distto (wg_graph0 0) (wg_nodes0 0) %a %a)"), 0);
    }

    #[test]
    fn wgraph_mst() {
        // Kruskal and Prim agree on the minimum spanning tree weight (11)
        assert_eq!(n("(wg_kruskal (wg_nodes0 0) (wg_edges0 0))"), 11);
        assert_eq!(n("(wg_prim (wg_graph0 0) (wg_nodes0 0))"), 11);
        // a trivial 3-node path a-b(1)-c(2): MST takes both edges, weight 3
        assert_eq!(n("(wg_kruskal [ %a [ %b [ %c 0 ] ] ] [ [ %a [ %b [ 1 0 ] ] ] [ [ %b [ %c [ 2 0 ] ] ] 0 ] ])"), 3);
    }

    #[test]
    fn numth_divisibility() {
        assert_eq!(n("(nt_gcd 48 36)"), 12);
        assert_eq!(n("(nt_gcd 17 5)"), 1);   // coprime
        assert_eq!(n("(nt_gcd 0 7)"), 7);    // gcd with 0
        assert_eq!(n("(nt_lcm 4 6)"), 12);
    }

    #[test]
    fn numth_primes() {
        assert_eq!(n("(nt_isprime 7)"), 0);  // prime
        assert_eq!(n("(nt_isprime 9)"), 1);  // composite
        assert_eq!(n("(nt_isprime 1)"), 1);  // 1 is not prime
        assert_eq!(n("(nt_isprime 2)"), 0);  // smallest prime
        // sieve of Eratosthenes up to 30
        assert_eq!(run("(nt_sieve 30)").unwrap(),
                   run("[2 [3 [5 [7 [11 [13 [17 [19 [23 [29 0]]]]]]]]]]").unwrap());
    }

    #[test]
    fn numth_modular_and_combinatorics() {
        assert_eq!(n("(nt_modpow 2 10 1000)"), 24);  // 1024 mod 1000
        assert_eq!(n("(nt_modpow 3 5 7)"), 5);       // 243 mod 7
        assert_eq!(n("(nt_fact 5)"), 120);
        assert_eq!(n("(nt_choose 5 2)"), 10);
        assert_eq!(n("(nt_choose 10 3)"), 120);
        assert_eq!(n("(nt_fib 10)"), 55);
        assert_eq!(n("(nt_fib 0)"), 0);
    }

    #[test]
    fn numth_digits() {
        assert_eq!(n("(nt_digitsum 1234)"), 10);
        assert_eq!(n("(nt_reverse 1234)"), 4321);
        assert_eq!(n("(nt_reverse 100)"), 1);   // trailing zeros drop
    }

    #[test]
    fn std_bitwise_ops() {
        // the new std bitwise vocabulary
        assert_eq!(n("(shl 1 4)"), 16);
        assert_eq!(n("(shr 48 2)"), 12);
        assert_eq!(n("(bit 6 1)"), 1);
        assert_eq!(n("(band 12 10)"), 8);   // 1100 & 1010 = 1000
        assert_eq!(n("(bor 12 10)"), 14);   // 1100 | 1010 = 1110
        assert_eq!(n("(bxor 12 10)"), 6);   // 1100 ^ 1010 = 0110
        assert_eq!(n("(popcount 255)"), 8);
        assert_eq!(n("(lowbit 12)"), 4);    // lowest set bit of 1100
    }

    #[test]
    fn bits_xor_tricks() {
        assert_eq!(n("(bm_single (bm_dup0 0))"), 4);    // the unpaired value
        assert_eq!(n("(bm_missing (bm_gap0 0) 3)"), 2); // 0..3 missing 2
        assert_eq!(n("(bm_add 5 3)"), 8);               // add without +
        assert_eq!(n("(bm_add 123 456)"), 579);
    }

    #[test]
    fn bits_tests_and_writes() {
        assert_eq!(n("(bm_ispow2 16)"), 1);
        assert_eq!(n("(bm_ispow2 12)"), 0);
        assert_eq!(n("(bm_ispow2 1)"), 1);
        assert_eq!(n("(bm_set 0 3)"), 8);
        assert_eq!(n("(bm_clear 15 1)"), 13);
        assert_eq!(n("(bm_toggle 5 0)"), 4);
        assert_eq!(n("(bm_clearlow 12)"), 8);
    }

    #[test]
    fn bits_enumeration_and_counting() {
        // 2^3 subsets of a 3-element set
        assert_eq!(n("(len (bm_subsets (bm_set0 0)))"), 8);
        assert_eq!(n("(bm_hamming 4 1)"), 2); // 100 vs 001
        // counting-bits DP for 0..7
        assert_eq!(run("(bm_countbits 7)").unwrap(),
                   run("[0 [1 [1 [2 [1 [2 [2 [3 0]]]]]]]]").unwrap());
    }

    #[test]
    fn std_sort_and_bytes() {
        // general merge sort added to std
        assert_eq!(run("(sort [3 [1 [2 [5 [4 0]]]]])").unwrap(),
                   run("[1 [2 [3 [4 [5 0]]]]]").unwrap());
        assert_eq!(run("(sortby (fn [a b] -> (gte a b)) [1 [3 [2 0]]])").unwrap(),
                   run("[3 [2 [1 0]]]").unwrap());   // descending
        assert_eq!(run("(sort 0)").unwrap(), run("0").unwrap()); // empty
        // cord <-> byte-list conversion
        assert_eq!(run("(bytes %abc)").unwrap(), run("[97 [98 [99 0]]]").unwrap());
        assert_eq!(n("(frombytes [104 [105 0]])"), n("%hi"));
        assert_eq!(n("(frombytes (reverse (bytes %abc)))"), n("%cba")); // roundtrip + reverse
    }

    #[test]
    fn strings_analysis() {
        assert_eq!(n("(st_rev %hello)"), n("%olleh"));
        assert_eq!(n("(st_ispalin %racecar)"), 0); // palindrome
        assert_eq!(n("(st_ispalin %hello)"), 1);   // not
        assert_eq!(n("(st_count %hello %l)"), 2);
    }

    #[test]
    fn strings_anagrams() {
        assert_eq!(n("(st_anagram %listen %silent)"), 0); // anagrams
        assert_eq!(n("(st_anagram %hi %bye)"), 1);        // not
        assert_eq!(n("(st_firstuniq %hello)"), n("%h"));  // first non-repeating
    }

    #[test]
    fn strings_search() {
        // naive and Rabin-Karp must agree on the first-match index
        assert_eq!(n("(st_find %abracadabra %cad)"), 4);
        assert_eq!(n("(st_rabinkarp %abracadabra %cad)"), 4);
        assert_eq!(n("(st_rabinkarp %abracadabra %bra)"), 1);   // first of two occurrences
        assert_eq!(n("(st_find %abracadabra %xyz)"), 11);       // absent -> text length
        assert_eq!(n("(st_rabinkarp %abracadabra %xyz)"), 11);
        assert_eq!(n("(st_contains %abracadabra %cad)"), 0);
    }

    #[test]
    fn strings_families() {
        assert_eq!(n("(st_lcp [%flower [%flow [%flight 0]]])"), n("%fl"));
        assert_eq!(n("(st_lcp2 %flower %flow)"), n("%flow"));
        // run-length encode %aaabbc -> [[a 3][b 2][c 1]]
        assert_eq!(run("(st_rle %aaabbc)").unwrap(),
                   run("[[97 3] [[98 2] [[99 1] 0]]]").unwrap());
    }

    #[test]
    fn grid_connectivity() {
        assert_eq!(n("(gr_islands (gr_grid0 0))"), 3);              // sample has 3 islands
        assert_eq!(n("(gr_islands [ [1 0] 0 ])"), 1);              // single land cell
        assert_eq!(n("(gr_islands [ [0 0] 0 ])"), 0);              // single water cell
        assert_eq!(n("(gr_islands 0)"), 0);                         // empty grid (edge case)
        // a 2x2 of all land is one island; all water is none
        assert_eq!(n("(gr_islands [ [1 [1 0]] [ [1 [1 0]] 0 ] ])"), 1);
        assert_eq!(n("(gr_islands [ [0 [0 0]] [ [0 [0 0]] 0 ] ])"), 0);
        // a diagonal touches only at corners -> NOT 4-connected, so two islands
        assert_eq!(n("(gr_islands [ [1 [0 0]] [ [0 [1 0]] 0 ] ])"), 2);
    }

    #[test]
    fn grid_pathcount() {
        assert_eq!(n("(gr_pathcount 3 3)"), 6);   // C(4,2)
        assert_eq!(n("(gr_pathcount 3 7)"), 28);  // C(8,2)
        assert_eq!(n("(gr_pathcount 1 5)"), 1);   // single row: one path
        assert_eq!(n("(gr_pathcount 5 1)"), 1);   // single column: one path
        assert_eq!(n("(gr_pathcount 0 5)"), 0);   // degenerate (edge case)
    }

    #[test]
    fn grid_transforms() {
        // transpose then transpose again is the identity
        assert_eq!(run("(gr_transpose (gr_transpose (gr_mat0 0)))").unwrap(),
                   run("(gr_mat0 0)").unwrap());
        // rotate 90 clockwise: top row becomes the right column
        assert_eq!(run("(gr_rotate (gr_mat0 0))").unwrap(),
                   run("[[7 [4 [1 0]]] [[8 [5 [2 0]]] [[9 [6 [3 0]]] 0]]]").unwrap());
        // spiral order of 1..9
        assert_eq!(run("(gr_spiral (gr_mat0 0))").unwrap(),
                   run("[1 [2 [3 [6 [9 [8 [7 [4 [5 0]]]]]]]]]").unwrap());
        // spiral edge cases: single row / single column flatten in order
        assert_eq!(run("(gr_spiral [ [1 [2 [3 0]]] 0 ])").unwrap(),
                   run("[1 [2 [3 0]]]").unwrap());
        assert_eq!(run("(gr_spiral [ [1 0] [ [2 0] [ [3 0] 0 ] ] ])").unwrap(),
                   run("[1 [2 [3 0]]]").unwrap());
        assert_eq!(run("(gr_transpose 0)").unwrap(), run("0").unwrap()); // empty
    }

    #[test]
    fn design_minstack() {
        // the minimum tracks correctly as values are pushed and popped
        assert_eq!(n("(dz_mstop (dz_ms0 0))"), 1);  // top after pushing 5,2,7,1
        assert_eq!(n("(dz_msmin (dz_ms0 0))"), 1);  // current min
        assert_eq!(n("(dz_msmin (dz_mspop (dz_ms0 0)))"), 2); // pop 1 -> min back to 2
        assert_eq!(n("(dz_mstop (dz_mspop (dz_ms0 0)))"), 7); // top is now 7
        // popping back to a single element: its min equals that element
        assert_eq!(n("(dz_msmin (dz_mspop (dz_mspop (dz_mspop (dz_ms0 0)))))"), 5);
    }

    #[test]
    fn design_queue_is_fifo() {
        // dequeues must come out in enqueue order (1,2,3) despite stack-based storage
        let q0 = "(dz_q0 0)";
        assert_eq!(n(&format!("(dz_qpeek {})", q0)), 1);
        assert_eq!(n(&format!("(head (dz_deq {}))", q0)), 1);
        assert_eq!(n(&format!("(head (dz_deq (tail (dz_deq {}))))", q0)), 2);
        assert_eq!(n(&format!("(head (dz_deq (tail (dz_deq (tail (dz_deq {}))))))", q0)), 3);
        // enqueue can interleave with dequeue and still preserve order
        assert_eq!(n("(head (dz_deq (dz_enq (tail (dz_deq (dz_q0 0))) 4)))"), 2);
    }

    #[test]
    fn design_lru_eviction_and_recency() {
        let c = "(dz_lru0 0)"; // cap-2 cache with a=1,b=2,c=3 put in order (a evicted)
        assert_eq!(run(&format!("(dz_lrukeys {})", c)).unwrap(),
                   run("[%c [%b 0]]").unwrap());           // most-recent first
        assert_eq!(n(&format!("(dz_lruget {} %a)", c)), 0); // a was evicted
        assert_eq!(n(&format!("(dz_lruget {} %b)", c)), 2);
        // touching b makes c the least-recently-used; a subsequent put evicts c, not b
        assert_eq!(run(&format!("(dz_lrukeys (dz_lruput (dz_lrutouch {} %b) %d 4))", c)).unwrap(),
                   run("[%d [%b 0]]").unwrap());
        // updating an existing key refreshes its value without growing the cache
        assert_eq!(n("(dz_lruget (dz_lruput (dz_lru0 0) %b 9) %b)"), 9);
    }

    #[test]
    fn trees_traversals() {
        let t = "(tr_tree0 0)";
        assert_eq!(run(&format!("(tr_inorder {})", t)).unwrap(),
                   run("[2 [3 [4 [5 [7 [8 [9 0]]]]]]]").unwrap());   // sorted (BST)
        assert_eq!(run(&format!("(tr_preorder {})", t)).unwrap(),
                   run("[5 [3 [2 [4 [8 [7 [9 0]]]]]]]").unwrap());   // root first
        assert_eq!(run(&format!("(tr_postorder {})", t)).unwrap(),
                   run("[2 [4 [3 [7 [9 [8 [5 0]]]]]]]").unwrap());   // root last
        assert_eq!(run(&format!("(tr_levelorder {})", t)).unwrap(),
                   run("[5 [3 [8 [2 [4 [7 [9 0]]]]]]]").unwrap());   // breadth-first
        // empty tree traverses to nothing
        assert_eq!(run("(tr_inorder 0)").unwrap(), run("0").unwrap());
    }

    #[test]
    fn trees_shape() {
        let t = "(tr_tree0 0)";
        assert_eq!(n(&format!("(tr_height {})", t)), 3);
        assert_eq!(n(&format!("(tr_count {})", t)), 7);
        assert_eq!(n("(tr_height 0)"), 0);              // empty
        assert_eq!(n("(tr_height (tr_leaf 9))"), 1);    // single leaf
        // invert is an involution: inverting twice restores the original
        assert_eq!(run(&format!("(tr_invert (tr_invert {}))", t)).unwrap(),
                   run(t).unwrap());
        // inverting mirrors the in-order traversal
        assert_eq!(run(&format!("(tr_inorder (tr_invert {}))", t)).unwrap(),
                   run("[9 [8 [7 [5 [4 [3 [2 0]]]]]]]").unwrap());
    }

    #[test]
    fn trees_paths_and_queries() {
        let t = "(tr_tree0 0)";
        assert_eq!(n(&format!("(tr_maxpath {})", t)), 22); // 5 + 8 + 9
        // lowest common ancestor across the structural cases
        assert_eq!(n(&format!("(tr_lca {} 2 4)", t)), 3);  // both under the left subtree
        assert_eq!(n(&format!("(tr_lca {} 7 9)", t)), 8);  // both under the right subtree
        assert_eq!(n(&format!("(tr_lca {} 2 7)", t)), 5);  // split at the root
        assert_eq!(n(&format!("(tr_lca {} 5 9)", t)), 5);  // one target ancestor of the other
        // validate-BST via in-order = strictly increasing
        assert_eq!(n(&format!("(tr_isbst {})", t)), 0);    // sorted -> is a BST
        assert_eq!(n("(tr_isbst (tr_bad0 0))"), 1);        // out of order -> not a BST
    }

    #[test]
    fn dp_one_dimensional() {
        // climbing stairs follows Fibonacci: 1,1,2,3,5,8,...
        assert_eq!(n("(dp_stairs 1)"), 1);
        assert_eq!(n("(dp_stairs 2)"), 2);
        assert_eq!(n("(dp_stairs 5)"), 8);
        // house robber: best non-adjacent sum of [2 7 9 3 1] is 2+9+1 = 12
        assert_eq!(n("(dp_rob (dp_houses0 0))"), 12);
        assert_eq!(n("(dp_rob 0)"), 0);              // empty
        assert_eq!(n("(dp_rob [5 0])"), 5);          // single house
    }

    #[test]
    fn dp_feasibility() {
        assert_eq!(n("(dp_subsetsum (dp_set0 0) 9)"), 0);    // 4+5 = 9: reachable
        assert_eq!(n("(dp_subsetsum (dp_set0 0) 100)"), 1);  // too big: not reachable
        assert_eq!(n("(dp_subsetsum (dp_set0 0) 0)"), 0);    // empty subset reaches 0
        assert_eq!(n("(dp_canpartition (dp_part0 0))"), 0);  // {1,5,5} vs {11}: equal halves
        assert_eq!(n("(dp_canpartition [1 [2 [5 0]]])"), 1); // total 8, no subset sums to 4
    }

    #[test]
    fn dp_counting_coinways() {
        // making 5 from {1,2,5}: {5},{2,2,1},{2,1,1,1},{1x5} = 4 combinations
        assert_eq!(n("(dp_coinways (dp_coins0 0) 5)"), 4);
        assert_eq!(n("(dp_coinways (dp_coins0 0) 0)"), 1);   // one way to make nothing
        assert_eq!(n("(dp_coinways (dp_coins0 0) 3)"), 2);   // {1,2} and {1,1,1}
    }

    #[test]
    fn dp_optimisation() {
        // 0/1 knapsack: items (2,3)(3,4)(4,5)(5,6), cap 5 -> take (2,3)+(3,4) = value 7
        assert_eq!(n("(dp_knapsack (dp_items0 0) 5)"), 7);
        assert_eq!(n("(dp_knapsack (dp_items0 0) 0)"), 0);   // no capacity
        // longest common subsequence
        assert_eq!(n("(dp_lcs %abcde %ace)"), 3);            // a,c,e
        assert_eq!(n("(dp_lcs %AGGTAB %GXTXAYB)"), 4);       // G,T,A,B
        assert_eq!(n("(dp_lcs %abc %xyz)"), 0);              // nothing in common
        assert_eq!(n("(dp_lcs %abc %abc)"), 3);              // identical strings
    }

    #[test]
    fn intervals_merging() {
        // overlapping set collapses to three disjoint intervals
        assert_eq!(run("(iv_merge (iv_sample0 0))").unwrap(),
                   run("[ [1 6] [ [8 10] [ [15 18] 0 ] ] ]").unwrap());
        // merge works regardless of input order (it sorts first)
        assert_eq!(run("(iv_merge [ [8 10] [ [1 3] [ [2 6] 0 ] ] ])").unwrap(),
                   run("[ [1 6] [ [8 10] 0 ] ]").unwrap());
        // touching endpoints DO merge ([1 2] and [2 3] -> [1 3])
        assert_eq!(run("(iv_merge [ [1 2] [ [2 3] 0 ] ])").unwrap(),
                   run("[ [1 3] 0 ]").unwrap());
        // insert into a sorted disjoint list, absorbing the overlaps
        assert_eq!(run("(iv_insert [ [1 2] [ [3 5] [ [6 7] [ [8 10] [ [12 16] 0 ] ] ] ] ] [4 8])").unwrap(),
                   run("[ [1 2] [ [3 10] [ [12 16] 0 ] ] ]").unwrap());
        // insert past the end appends
        assert_eq!(run("(iv_insert [ [1 2] [ [3 5] 0 ] ] [10 12])").unwrap(),
                   run("[ [1 2] [ [3 5] [ [10 12] 0 ] ] ]").unwrap());
    }

    #[test]
    fn intervals_scheduling() {
        // [0 30] overlaps the others, so one person cannot attend all
        assert_eq!(n("(iv_canattend (iv_meetings0 0))"), 1);
        assert_eq!(n("(iv_canattend [ [1 2] [ [3 4] 0 ] ])"), 0);   // disjoint: fine
        assert_eq!(n("(iv_canattend [ [1 5] [ [5 8] 0 ] ])"), 0);   // touching is not a conflict
        // minimum rooms = peak concurrency
        assert_eq!(n("(iv_minrooms (iv_meetings0 0))"), 2);
        assert_eq!(n("(iv_minrooms [ [1 10] [ [2 10] [ [3 10] 0 ] ] ])"), 3); // all overlap
        assert_eq!(n("(iv_minrooms [ [1 2] [ [3 4] 0 ] ])"), 1);             // sequential
    }

    #[test]
    fn intervals_setops() {
        // intersection matches the canonical two-pointer result
        assert_eq!(run("(iv_intersect (iv_a0 0) (iv_b0 0))").unwrap(),
                   run("[ [1 2] [ [5 5] [ [8 10] [ [15 23] [ [24 24] [ [25 25] 0 ] ] ] ] ] ]").unwrap());
        // disjoint lists intersect to nothing
        assert_eq!(run("(iv_intersect [ [1 2] 0 ] [ [5 6] 0 ])").unwrap(), run("0").unwrap());
        // covered length sums the merged widths: (6-1)+(10-8)+(18-15) = 10
        assert_eq!(n("(iv_coverage (iv_sample0 0))"), 10);
    }

    #[test]
    fn search_boundaries() {
        let s = "(se_sorted0 0)";  // [1 2 2 2 3 4]
        assert_eq!(n(&format!("(se_lowerbound {} 2)", s)), 1);  // first 2 at index 1
        assert_eq!(n(&format!("(se_upperbound {} 2)", s)), 4);  // one past the last 2
        assert_eq!(n(&format!("(se_count {} 2)", s)), 3);       // three 2s
        assert_eq!(n(&format!("(se_count {} 5)", s)), 0);       // absent -> 0
        assert_eq!(n(&format!("(se_contains {} 3)", s)), 0);    // present
        assert_eq!(n(&format!("(se_contains {} 9)", s)), 1);    // absent
        assert_eq!(n(&format!("(se_lowerbound {} 0)", s)), 0);  // before everything
        assert_eq!(n(&format!("(se_lowerbound {} 9)", s)), 6);  // past everything (len)
    }

    #[test]
    fn search_rotated() {
        let r = "(se_rot0 0)";  // [4 5 6 7 0 1 2]
        assert_eq!(n(&format!("(se_rotated {} 0)", r)), 4);
        assert_eq!(n(&format!("(se_rotated {} 4)", r)), 0);     // pivot value at front
        assert_eq!(n(&format!("(se_rotated {} 2)", r)), 6);     // last element
        assert_eq!(n(&format!("(se_rotated {} 9)", r)), 7);     // absent -> length
        assert_eq!(n(&format!("(se_rotmin {})", r)), 0);        // rotation point value
        assert_eq!(n("(se_rotmin [1 [2 [3 0]]])"), 1);          // not actually rotated
        assert_eq!(n("(se_rotmin [5 0])"), 5);                  // single element
    }

    #[test]
    fn search_slope_and_answer() {
        // a peak (strictly greater than neighbours)
        assert_eq!(n("(se_peak (se_peak0 0))"), 2);             // value 3 at index 2
        // integer square root via binary search on the answer
        assert_eq!(n("(se_isqrt 16)"), 4);
        assert_eq!(n("(se_isqrt 17)"), 4);                      // floor
        assert_eq!(n("(se_isqrt 24)"), 4);
        assert_eq!(n("(se_isqrt 25)"), 5);
        assert_eq!(n("(se_isqrt 0)"), 0);
        assert_eq!(n("(se_isqrt 1)"), 1);
        // Koko bananas: hours needed, and the minimum feasible speed
        assert_eq!(n("(se_hours (se_piles0 0) 4)"), 8);         // exactly 8 hours at speed 4
        assert_eq!(n("(se_minspeed (se_piles0 0) 8)"), 4);      // smallest speed within 8h
        assert_eq!(n("(se_minspeed (se_piles0 0) 100)"), 1);    // lots of time -> speed 1
    }

    #[test]
    fn graphs_ordering() {
        // Kahn topological order of the CLRS DAG respects every edge
        assert_eq!(run("(gp_toposort (gp_dag0 0))").unwrap(),
                   run("[ 5 [ 4 [ 2 [ 3 [ 0 [ 1 0 ] ] ] ] ] ]").unwrap());
        assert_eq!(n("(gp_isdag (gp_dag0 0))"), 0);            // acyclic
        assert_eq!(n("(gp_isdag (gp_cyc0 0))"), 1);            // 0->1->2->0 has a cycle
        // a cyclic graph yields no full ordering (Kahn emits nothing here)
        assert_eq!(run("(gp_toposort (gp_cyc0 0))").unwrap(), run("0").unwrap());
        // in-degree counting (only edges from remaining nodes)
        assert_eq!(n("(gp_indegR (gp_dag0 0) 0 0)"), 2);       // 0 has prerequisites 5 and 4
        assert_eq!(n("(gp_indegR (gp_dag0 0) 5 0)"), 0);       // 5 is a source
    }

    #[test]
    fn graphs_components_reach() {
        assert_eq!(n("(gp_components (gp_undir0 0))"), 3);     // {0,1,2}{3,4}{5}
        assert_eq!(n("(gp_reach (gp_undir0 0) 0 2)"), 0);     // same component
        assert_eq!(n("(gp_reach (gp_undir0 0) 0 3)"), 1);     // different component
        assert_eq!(n("(gp_reach (gp_undir0 0) 0 0)"), 0);     // a node reaches itself
        assert_eq!(n("(gp_reach (gp_undir0 0) 3 4)"), 0);     // the small edge
    }

    #[test]
    fn graphs_bipartite() {
        assert_eq!(n("(gp_bipartite (gp_bip0 0))"), 0);       // even cycle: 2-colourable
        assert_eq!(n("(gp_bipartite (gp_odd0 0))"), 1);       // triangle: odd cycle, not bipartite
        // undir0's {0,1,2} piece is a triangle, so the whole graph is not bipartite
        assert_eq!(n("(gp_bipartite (gp_undir0 0))"), 1);
        // a simple path 0-1-2 (no odd cycle) IS bipartite
        assert_eq!(n("(gp_bipartite [ [0 [1 0]] [ [1 [0 [2 0]]] [ [2 [1 0]] 0 ] ] ])"), 0);
    }

    #[test]
    fn backtrack_parens_and_combos() {
        // counts of valid parenthesisations are the Catalan numbers 1,2,5,14
        assert_eq!(n("(len (bk_parens 1))"), 1);
        assert_eq!(n("(len (bk_parens 2))"), 2);
        assert_eq!(n("(len (bk_parens 3))"), 5);
        assert_eq!(n("(len (bk_parens 4))"), 14);
        // combination sum of {2,3,6,7} to 7 -> {2,2,3} and {7}
        assert_eq!(run("(bk_combsum (bk_cands0 0) 7)").unwrap(),
                   run("[ [2 [2 [3 0]]] [ [7 0] 0 ] ]").unwrap());
        // all 2-combinations of {1..4}
        assert_eq!(run("(bk_combine 4 2)").unwrap(),
                   run("[ [1 [2 0]] [ [1 [3 0]] [ [1 [4 0]] [ [2 [3 0]] [ [2 [4 0]] [ [3 [4 0]] 0 ] ] ] ] ] ]").unwrap());
        assert_eq!(n("(len (bk_combine 5 3))"), 10);          // C(5,3) = 10
        assert_eq!(n("(len (bk_combine 6 0))"), 1);           // C(n,0) = 1 (the empty set)
    }

    #[test]
    fn backtrack_partition_and_grid() {
        // palindrome partitions: aab -> 2, aaa -> 4
        assert_eq!(n("(len (bk_partition %aab))"), 2);
        assert_eq!(n("(len (bk_partition %aaa))"), 4);
        assert_eq!(n("(bk_ispal (bytes %racecar))"), 0);      // palindrome
        assert_eq!(n("(bk_ispal (bytes %abca))"), 1);         // not
        // word search on the classic board ABCE / SFCS / ADEE
        assert_eq!(n("(bk_wordsearch (bk_grid0 0) %ABCCED)"), 0);  // present (snaking path)
        assert_eq!(n("(bk_wordsearch (bk_grid0 0) %SEE)"), 0);     // present
        assert_eq!(n("(bk_wordsearch (bk_grid0 0) %ABCB)"), 1);    // absent: would reuse a cell
        assert_eq!(n("(bk_wordsearch (bk_grid0 0) %SFCS)"), 0);    // straight across row 2
    }

    #[test]
    fn greedy_jump_and_gas() {
        assert_eq!(n("(gd_canjump (gd_jump0 0))"), 0);       // [2 3 1 1 4] reachable
        assert_eq!(n("(gd_canjump (gd_jump1 0))"), 1);       // [3 2 1 0 4] stuck
        assert_eq!(n("(gd_canjump [0 0])"), 0);              // single element: already there
        assert_eq!(n("(gd_minjumps (gd_jump0 0))"), 2);      // 0->1->4
        assert_eq!(n("(gd_minjumps [0 0])"), 0);             // single element
        assert_eq!(n("(gd_minjumps [2 [1 0]])"), 1);         // one jump suffices
        // gas station: valid start is index 3; an impossible loop returns n (=3)
        assert_eq!(n("(gd_gas (gd_gasG0 0) (gd_gasC0 0))"), 3);
        assert_eq!(n("(gd_gas [2 [3 [4 0]]] [3 [4 [3 0]]])"), 3);  // total gas < cost
        assert_eq!(n("(gd_gas [5 [1 [2 [3 [4 0]]]]] [4 [4 [1 [5 [1 0]]]]])"), 4);  // start 4
    }

    #[test]
    fn greedy_scan_and_schedule() {
        // partition labels
        assert_eq!(run("(gd_partition %abac)").unwrap(), run("[ 3 [ 1 0 ] ]").unwrap());
        assert_eq!(run("(gd_partition %ababcbacadefegdehijhklij)").unwrap(),
                   run("[ 9 [ 7 [ 8 0 ] ] ]").unwrap());   // the classic example
        // candy: two-pass minimum
        assert_eq!(n("(gd_candy (gd_rat0 0))"), 5);          // [1 0 2] -> 2,1,2
        assert_eq!(n("(gd_candy [1 [2 [2 0]]])"), 4);        // [1 2 2] -> 1,2,1
        assert_eq!(n("(gd_candy [1 [3 [2 [2 [1 0]]]]])"), 7); // 1,2,1,... descent
        assert_eq!(n("(gd_candy [5 0])"), 1);                // single child
        // task scheduler: peak frequency paces the timeline
        assert_eq!(n("(gd_taskschedule (gd_freqs0 0) 2)"), 8);  // AB_AB_AB
        assert_eq!(n("(gd_taskschedule (gd_freqs0 0) 0)"), 6);  // no cooldown: just run them
        assert_eq!(n("(gd_taskschedule [6 [1 [1 [1 [1 [1 0]]]]]] 2)"), 16); // one dominant task
    }

    #[test]
    fn strings_search_crosscheck() {
        // the naive and Rabin-Karp searches must return the SAME index for every
        // pattern: matches at the start, middle, end, the whole string, and absent.
        for pat in ["%a", "%abr", "%bra", "%dabra", "%abracadabra",
                    "%z", "%abrx", "%cadabra", "%aa"] {
            let naive = n(&format!("(st_find %abracadabra {})", pat));
            let rk = n(&format!("(st_rabinkarp %abracadabra {})", pat));
            assert_eq!(naive, rk, "naive vs Rabin-Karp disagree on {}", pat);
        }
    }

    #[test]
    fn std_sort_properties() {
        // idempotent: sorting an already-sorted list changes nothing
        assert_eq!(run("(sort (sort [5 [3 [5 [1 [3 0]]]]]))").unwrap(),
                   run("(sort [5 [3 [5 [1 [3 0]]]]])").unwrap());
        // duplicates are kept, and the result is ordered
        assert_eq!(run("(sort [3 [1 [2 [1 [3 0]]]]])").unwrap(),
                   run("[1 [1 [2 [3 [3 0]]]]]").unwrap());
        // reverse comparator yields descending order
        assert_eq!(run("(sortby (fn [a b] -> (gte a b)) [1 [2 [3 0]]])").unwrap(),
                   run("[3 [2 [1 0]]]").unwrap());
        // single element and empty are fixed points
        assert_eq!(run("(sort [7 0])").unwrap(), run("[7 0]").unwrap());
        assert_eq!(run("(sort 0)").unwrap(), run("0").unwrap());
    }

    #[test]
    fn numth_sieve_consistency() {
        // prime-counting: pi(30) = 10, pi(50) = 15
        assert_eq!(n("(len (nt_sieve 30))"), 10);
        assert_eq!(n("(len (nt_sieve 50))"), 15);
        // every number the sieve reports is confirmed prime by the independent test
        // (nt_isprime returns 0 for prime, so the sum over all sieve outputs must be 0)
        assert_eq!(n("(foldl (fn [acc p] -> (add acc (nt_isprime p))) 0 (nt_sieve 50))"), 0);
    }

    #[test]
    fn bits_add_crosscheck() {
        // bm_add (XOR/AND/shift) must match ordinary addition across a range of values
        for &(a, b) in &[(0u32, 0u32), (1, 1), (7, 8), (255, 1), (123, 456), (1000, 24)] {
            assert_eq!(n(&format!("(bm_add {} {})", a, b)), (a + b) as u128,
                       "bm_add wrong for {}+{}", a, b);
        }
    }

    #[test]
    fn dhash_bit_primitives() {
        // dh_set/dh_bit/dh_popcount now delegate to std bit/bor/shl/popcount; pin behavior.
        assert_eq!(n("(dh_bit 6 1)"), 1);          // bit 1 of 110 is set
        assert_eq!(n("(dh_bit 6 0)"), 0);          // bit 0 of 110 is clear
        assert_eq!(n("(dh_set 0 5)"), 32);         // setting bit 5 of 0 -> 2^5
        assert_eq!(n("(dh_set 32 5)"), 32);        // idempotent
        assert_eq!(n("(dh_bit (dh_set (dh_set 0 3) 7) 7)"), 1);
        assert_eq!(n("(dh_popcount 255)"), 8);
        assert_eq!(n("(dh_popcount 0)"), 0);
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
        // the canonical contrast: snapshot isolation ALLOWS the doctors-on-call write
        // skew (second txn commits), serializable read-set validation PREVENTS it.
        assert_eq!(run("(db_skew_si 0)").unwrap(), crate::knot::cord("commit"));
        assert_eq!(run("(db_skew_ser 0)").unwrap(), crate::knot::cord("abort"));
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
