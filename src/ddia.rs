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
        println!("        mvcc hll cms stream raft");
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
