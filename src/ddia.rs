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
    }

    if all || topic == "btree" {
        head("B-tree (Ch.3)", "balanced high-fanout index, updated in place, O(log n) reads — the read-optimized family");
        let p = "[[%mm 1] [[%dd 2] [[%tt 3] [[%aa 4] [[%gg 5] [[%pp 6] [[%cc 7] [[%kk 8] [[%ww 9] [[%bb 10] [[%ff 11] [[%zz 12] 0]]]]]]]]]]]]";
        let t = format!("(bt_build {} 2)", p);
        row("in-order scan (sorted)", &format!("(map (fn [e] -> (head e)) (bt_inorder {}))", t));
        row("height (12 keys, t=2)", &format!("(bt_height {})", t));
        row("search aa", &format!("(bt_search {} %aa)", t));
        row("search qq (absent)", &format!("(bt_search {} %qq)", t));
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

    if all {
        println!("\nEach technique is a Latte module in lib/ (namespaced: dh_/bloom_/lsm_/bt_/vc_/");
        println!("gc_/lam_/ch_/q_/w_/mr_/mk_). See docs/data-intensive.md for the interview guide.");
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
    fn btree_inorder_is_sorted_and_searchable() {
        let p = "[[%mm 1] [[%aa 2] [[%zz 3] [[%dd 4] [[%kk 5] 0]]]]]";
        let t = format!("(bt_build {} 2)", p);
        assert_eq!(n(&format!("(len (bt_inorder {}))", t)), 5);
        // bt_search returns [%val v]; pull the value for %zz
        assert_eq!(n(&format!("(tail (bt_search {} %zz))", t)), 3);
        assert_eq!(n(&format!("(bt_search {} %qq)", t)), 0); // absent
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
}
