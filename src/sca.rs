//! SCArs — the Orpheus sound-change applier.
//!
//! A general, featureful SCA. The matching engine (`lib/sca.lat`) runs on Loom; this
//! host compiles rules down to it and adds the prosodic passes (breaking/nasalization)
//! that need syllable/stress structure.
//!
//! Features: multi-segment FROM/TO (clusters, metathesis), multi-segment context with
//! left/right environments, word boundary `#`, wildcard `*` (any one segment), deletion
//! (empty TO), insertion (empty FROM), multi-character graphemes (longest-match
//! tokenization), phoneme categories (named `class`es), and **category correspondence**
//! (`STOPV > STOPVD` maps p→b, t→d, k→g by index).
//!
//! Rule syntax:  FROM > TO / PRE _ POST   (tokens are whitespace-separated; `#`
//! is a boundary, `*` a wildcard, a class name a category; omit `/…` for no context).

use crate::atom::Atom;
use crate::knot::{cell, cord, num, Knot, N};
use crate::latte;
use crate::loom::{self, slot, tar};
use std::collections::HashMap;
use std::sync::OnceLock;

pub const SCA_SRC: &str = include_str!("../lib/sca.lat");
pub const LIGURIAN_SCA: &str = include_str!("../lib/ligurian.sca");

// ----- tagged tokens & segments ---------------------------------------------
#[derive(Clone, Debug)]
enum Tok {
    Lit(String),
    Wild,
    Bound,
}

fn tok_knot(t: &Tok) -> N {
    match t {
        Tok::Lit(s) => cell(num(0), cord(s)),
        Tok::Wild => cell(num(1), num(0)),
        Tok::Bound => cell(num(2), num(0)),
    }
}

fn toks_to_list(ts: &[Tok]) -> N {
    let mut acc = num(0);
    for t in ts.iter().rev() {
        acc = cell(tok_knot(t), acc);
    }
    acc
}

fn phon_seg(g: &str) -> N {
    cell(num(0), cord(g))
}
fn bound_seg() -> N {
    cell(num(2), num(0))
}

/// Boundary-padded segment list for a word, tokenized by the grapheme set.
fn word_segments(graphemes: &[String], s: &str) -> N {
    let mut segs: Vec<N> = vec![bound_seg()];
    for g in tokenize(graphemes, s) {
        segs.push(phon_seg(&g));
    }
    segs.push(bound_seg());
    let mut acc = num(0);
    for seg in segs.into_iter().rev() {
        acc = cell(seg, acc);
    }
    acc
}

/// Longest-match tokenization against the grapheme set (falls back to one scalar).
fn tokenize(graphemes: &[String], s: &str) -> Vec<String> {
    let multi: Vec<&String> = graphemes.iter().filter(|g| g.chars().count() > 1).collect();
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let mut matched: Option<String> = None;
        // try longest multi-char grapheme first
        for g in &multi {
            let gl = g.chars().count();
            if i + gl <= cs.len() && cs[i..i + gl].iter().collect::<String>() == ***g {
                matched = Some((*g).clone());
                break;
            }
        }
        match matched {
            Some(g) => {
                let gl = g.chars().count();
                out.push(g);
                i += gl;
            }
            None => {
                out.push(cs[i].to_string());
                i += 1;
            }
        }
    }
    out
}

/// Turn a result segment list back into a string ('#' for the boundary padding).
fn seglist_to_string(k: &N) -> String {
    let mut out = String::new();
    let mut cur = k.clone();
    loop {
        match &*cur {
            Knot::Cell(seg, t) => {
                if let Knot::Cell(tag, payload) = &**seg {
                    if let Some(a) = tag.as_atom() {
                        if a.to_u128() == Some(2) {
                            out.push('#');
                        } else if let Some(c) = payload.as_atom().and_then(|p| p.as_cord()) {
                            out.push_str(&c);
                        }
                    }
                }
                cur = t.clone();
            }
            Knot::Atom(_) => break,
        }
    }
    out
}

// ----- rule parsing ---------------------------------------------------------
#[derive(Clone, Debug)]
enum TokSpec {
    Lit(String),
    Cat(String),
    Wild,
    Bound,
}

fn parse_part(part: &str, classes: &HashMap<String, Vec<String>>) -> Vec<TokSpec> {
    part.split_whitespace()
        .map(|t| {
            if t == "#" {
                TokSpec::Bound
            } else if t == "*" {
                TokSpec::Wild
            } else if classes.contains_key(t) {
                TokSpec::Cat(t.to_string())
            } else {
                TokSpec::Lit(t.to_string())
            }
        })
        .collect()
}

type Concrete = (Vec<Tok>, Vec<Tok>, Vec<Tok>, Vec<Tok>);

/// Expand categories + correspondence in one rule into concrete literal rules.
fn expand_rule(
    from: &[TokSpec],
    to: &[TokSpec],
    pre: &[TokSpec],
    post: &[TokSpec],
    classes: &HashMap<String, Vec<String>>,
) -> Result<Vec<Concrete>, String> {
    let cats_in = |xs: &[TokSpec]| -> Vec<usize> {
        xs.iter().enumerate().filter(|(_, t)| matches!(t, TokSpec::Cat(_))).map(|(i, _)| i).collect()
    };
    let name_at = |xs: &[TokSpec], i: usize| match &xs[i] {
        TokSpec::Cat(n) => n.clone(),
        _ => unreachable!(),
    };
    let fcats = cats_in(from);
    let tcats = cats_in(to);
    let precats = cats_in(pre);
    let postcats = cats_in(post);
    let npair = fcats.len().min(tcats.len());
    if tcats.len() > npair {
        return Err("output category with no matching input category".into());
    }
    // each "slot" yields, per option, a set of substitutions (which list, position, literal)
    #[derive(Clone)]
    enum Tgt {
        From,
        To,
        Pre,
        Post,
    }
    let mut slots: Vec<Vec<Vec<(Tgt, usize, String)>>> = Vec::new();
    // correspondence pairs
    for i in 0..npair {
        let fn_ = name_at(from, fcats[i]);
        let tn_ = name_at(to, tcats[i]);
        let fm = classes.get(&fn_).ok_or(format!("unknown class {}", fn_))?;
        let tm = classes.get(&tn_).ok_or(format!("unknown class {}", tn_))?;
        if fm.len() != tm.len() {
            return Err(format!("classes {} and {} differ in length", fn_, tn_));
        }
        let opts = (0..fm.len())
            .map(|k| vec![(Tgt::From, fcats[i], fm[k].clone()), (Tgt::To, tcats[i], tm[k].clone())])
            .collect();
        slots.push(opts);
    }
    // independent input categories (matched, not corresponded)
    for &p in &fcats[npair..] {
        let nm = name_at(from, p);
        let m = classes.get(&nm).ok_or(format!("unknown class {}", nm))?;
        slots.push(m.iter().map(|x| vec![(Tgt::From, p, x.clone())]).collect());
    }
    for &p in &precats {
        let nm = name_at(pre, p);
        let m = classes.get(&nm).ok_or(format!("unknown class {}", nm))?;
        slots.push(m.iter().map(|x| vec![(Tgt::Pre, p, x.clone())]).collect());
    }
    for &p in &postcats {
        let nm = name_at(post, p);
        let m = classes.get(&nm).ok_or(format!("unknown class {}", nm))?;
        slots.push(m.iter().map(|x| vec![(Tgt::Post, p, x.clone())]).collect());
    }

    // cartesian product over slot options
    let mut combos: Vec<Vec<(Tgt, usize, String)>> = vec![vec![]];
    for slot in &slots {
        let mut next = Vec::new();
        for combo in &combos {
            for opt in slot {
                let mut c = combo.clone();
                c.extend(opt.iter().cloned());
                next.push(c);
            }
        }
        combos = next;
    }

    let build = |specs: &[TokSpec], subs: &[(Tgt, usize, String)], want: u8| -> Vec<Tok> {
        let mut res: Vec<Option<String>> = vec![None; specs.len()];
        for (t, pos, lit) in subs {
            let m = matches!(
                (t, want),
                (Tgt::From, 0) | (Tgt::To, 1) | (Tgt::Pre, 2) | (Tgt::Post, 3)
            );
            if m {
                res[*pos] = Some(lit.clone());
            }
        }
        specs
            .iter()
            .enumerate()
            .map(|(i, sp)| match sp {
                TokSpec::Lit(s) => Tok::Lit(s.clone()),
                TokSpec::Wild => Tok::Wild,
                TokSpec::Bound => Tok::Bound,
                TokSpec::Cat(_) => Tok::Lit(res[i].clone().unwrap_or_default()),
            })
            .collect()
    };

    let mut out = Vec::new();
    for combo in &combos {
        out.push((
            build(from, combo, 0),
            build(to, combo, 1),
            build(pre, combo, 2),
            build(post, combo, 3),
        ));
    }
    Ok(out)
}

fn concrete_to_knot(c: &Concrete) -> N {
    let (from, to, pre, post) = c;
    let mut pre_rev = pre.clone();
    pre_rev.reverse(); // engine matches reversed pre against the reversed accumulator
    cell(
        toks_to_list(from),
        cell(toks_to_list(to), cell(toks_to_list(&pre_rev), toks_to_list(post))),
    )
}

/// Parse a `.sca` source (or a single compact rule) into concrete rules + graphemes.
pub fn parse_sca(src: &str) -> Result<(Vec<N>, Vec<String>), String> {
    let mut classes: HashMap<String, Vec<String>> = HashMap::new();
    let mut rules: Vec<N> = Vec::new();
    let mut graphemes: Vec<String> = Vec::new();
    let note = |g: &str, graphemes: &mut Vec<String>| {
        if !g.is_empty() && !graphemes.iter().any(|x| x == g) {
            graphemes.push(g.to_string());
        }
    };
    for raw in src.lines() {
        let line = raw.split("::").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("class ") {
            let (name, members) = rest.split_once('=').ok_or(format!("bad class: {}", line))?;
            let mems: Vec<String> = members.split_whitespace().map(|s| s.to_string()).collect();
            for m in &mems {
                note(m, &mut graphemes);
            }
            classes.insert(name.trim().to_string(), mems);
            continue;
        }
        let (from_s, rest) = line.split_once('>').ok_or(format!("rule missing '>': {}", line))?;
        let (to_s, ctx_s) = match rest.split_once('/') {
            Some((t, c)) => (t.trim(), c.trim()),
            None => (rest.trim(), ""),
        };
        let (pre_s, post_s) = if ctx_s.is_empty() {
            ("", "")
        } else {
            match ctx_s.split_once('_') {
                Some((p, q)) => (p.trim(), q.trim()),
                None => (ctx_s.trim(), ""),
            }
        };
        let from = parse_part(from_s.trim(), &classes);
        let to = parse_part(to_s, &classes);
        let pre = parse_part(pre_s, &classes);
        let post = parse_part(post_s, &classes);
        for specs in [&from, &to, &pre, &post] {
            for sp in specs {
                if let TokSpec::Lit(s) = sp {
                    note(s, &mut graphemes);
                }
            }
        }
        for c in expand_rule(&from, &to, &pre, &post, &classes)? {
            rules.push(concrete_to_knot(&c));
        }
    }
    Ok((rules, graphemes))
}

/// Back-compat: just the rules.
#[allow(dead_code)]
pub fn parse_sca_file(src: &str) -> Result<Vec<N>, String> {
    Ok(parse_sca(src)?.0)
}

fn rules_vec_to_list(rules: &[N]) -> N {
    let mut acc = num(0);
    for r in rules.iter().rev() {
        acc = cell(r.clone(), acc);
    }
    acc
}

// ----- the engine (compiled once) -------------------------------------------
fn engine() -> &'static (N, u128) {
    static E: OnceLock<(N, u128)> = OnceLock::new();
    E.get_or_init(|| {
        let (core, axes) = latte::compile_module(SCA_SRC).expect("SCArs engine compiles");
        let ax = axes
            .iter()
            .find(|(n, _)| n == "apply")
            .map(|(_, a)| *a)
            .expect("apply arm");
        (core, ax)
    })
}

fn run_engine(word_segs: N, rules_list: N) -> Result<N, String> {
    let (core, apply_axis) = engine();
    let sample = cell(word_segs, rules_list);
    let core2 = loom::edit(&Atom::from_u128(3), &sample, core).map_err(|e| format!("{:?}", e))?;
    let armf = slot(&Atom::from_u128(*apply_axis), &core2).map_err(|e| format!("{:?}", e))?;
    tar(&core2, &armf).map_err(|e| format!("{:?}", e))
}

/// Apply ad-hoc rules to a word (used by the CLI and Facet `SCArs.apply`).
pub fn run_sca(word: &str, rules: &[String]) -> Result<String, String> {
    let src = rules.join("\n");
    let (rule_knots, graphemes) = parse_sca(&src)?;
    let segs = word_segments(&graphemes, word);
    let out = run_engine(segs, rules_vec_to_list(&rule_knots))?;
    Ok(seglist_to_string(&out).replace('#', ""))
}

// ----- Ligurian Solar -> Heart ----------------------------------------------
fn ligurian() -> &'static (N, Vec<String>) {
    static L: OnceLock<(N, Vec<String>)> = OnceLock::new();
    L.get_or_init(|| {
        let (rules, graphemes) = parse_sca(LIGURIAN_SCA).expect("ligurian.sca parses");
        (rules_vec_to_list(&rules), graphemes)
    })
}

fn strip_stress(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            o => o,
        })
        .collect()
}

pub fn evolve(word: &str) -> Result<String, String> {
    let (rules_list, graphemes) = ligurian();
    let segs = word_segments(graphemes, &strip_stress(word));
    let out = run_engine(segs, rules_list.clone())?;
    let padded = seglist_to_string(&out); // includes '#' boundaries
    let chars: Vec<char> = padded.chars().collect();
    let chars = break_stressed(&chars);
    let chars = nasalize(&chars);
    let result: String = chars.into_iter().collect();
    Ok(result.replace('#', ""))
}

// ----- prosodic passes (Phase VI breaking, Phase VII nasalization) ----------
const VOWELS: &str = "aeiouāēīōū";
fn is_vowel(c: char) -> bool {
    VOWELS.contains(c)
}
fn is_cons(c: char) -> bool {
    c != '#' && !is_vowel(c)
}

fn nasalize(chars: &[char]) -> Vec<char> {
    let nasal_of = |c: char| -> Vec<char> {
        match c {
            'a' => vec!['ã'],
            'e' => vec!['ẽ'],
            'i' => vec!['ĩ'],
            'o' => vec!['õ'],
            'u' => vec!['ũ'],
            'ā' | 'ē' | 'ī' | 'ō' | 'ū' => vec![c, '\u{0303}'],
            o => vec![o],
        }
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_vowel(c)
            && i + 2 < chars.len()
            && (chars[i + 1] == 'm' || chars[i + 1] == 'n')
            && !is_vowel(chars[i + 2])
        {
            out.extend(nasal_of(c));
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn break_stressed(chars: &[char]) -> Vec<char> {
    let n = chars.len();
    let mut nuclei: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if is_vowel(chars[i]) {
            let s = i;
            while i < n && is_vowel(chars[i]) {
                i += 1;
            }
            nuclei.push((s, i));
        } else {
            i += 1;
        }
    }
    if nuclei.len() < 2 {
        return chars.to_vec();
    }
    let (ps, pe) = nuclei[nuclei.len() - 2];
    if pe - ps != 1 {
        return chars.to_vec();
    }
    let broken: &[char] = match chars[ps] {
        'a' => &['a', 'o'],
        'e' => &['e', 'a'],
        'o' => &['u', 'o'],
        _ => return chars.to_vec(),
    };
    if ps == 0 {
        return chars.to_vec();
    }
    let onset = chars[ps - 1];
    if onset == '#' || is_vowel(onset) || onset == 'v' || onset == 'ɣ' {
        return chars.to_vec();
    }
    let c1 = chars.get(pe).copied();
    let c2 = chars.get(pe + 1).copied();
    let open = match (c1, c2) {
        (Some('#'), _) => true,
        (Some(x), _) if is_vowel(x) => true,
        (Some(x), Some(y)) if is_cons(x) && is_vowel(y) => true,
        _ => false,
    };
    let liquid_c = matches!((c1, c2), (Some(x), Some(y)) if (x == 'r' || x == 'l') && is_cons(y));
    if !(open || liquid_c) {
        return chars.to_vec();
    }
    let mut out = Vec::with_capacity(n + 1);
    out.extend_from_slice(&chars[..ps]);
    out.extend_from_slice(broken);
    out.extend_from_slice(&chars[ps + 1..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sca(word: &str, rules: &[&str]) -> String {
        let owned: Vec<String> = rules.iter().map(|s| s.to_string()).collect();
        run_sca(word, &owned).expect("sca runs")
    }

    #[test]
    fn unconditional_substitution() {
        assert_eq!(sca("kasa", &["k>g"]), "gasa");
        assert_eq!(sca("banana", &["a>o"]), "bonono");
    }

    #[test]
    fn intervocalic_context() {
        assert_eq!(sca("asasa", &["s>z/a_a"]), "azaza");
        assert_eq!(sca("asla", &["s>z/a_a"]), "asla");
    }

    #[test]
    fn deletion_and_right_context() {
        assert_eq!(sca("anta", &["t>/n_"]), "ana");
        assert_eq!(sca("atap", &["a>e/_t"]), "etap");
    }

    #[test]
    fn multi_segment_cluster() {
        // a two-segment FROM and TO: "tt" -> "st" (Solar cluster change dt/tt > st)
        assert_eq!(sca("atta", &["t t > s t"]), "asta");
        // ending change: word-final "o s" -> "u"
        assert_eq!(sca("domos", &["o s > u / _ #"]), "domu");
    }

    #[test]
    fn metathesis() {
        // swap an "s p" cluster to "p s"
        assert_eq!(sca("aspa", &["s p > p s"]), "apsa");
    }

    #[test]
    fn insertion() {
        // epenthesize "a" between two specific consonants:  insert a / k _ t
        assert_eq!(sca("akt", &[" > a / k _ t"]), "akat");
    }

    #[test]
    fn wildcard() {
        // delete any segment between two a's:  * > 0 / a _ a   (here the 'x')
        assert_eq!(sca("axa", &["* > / a _ a"]), "aa");
    }

    #[test]
    fn category_correspondence() {
        // STOPV -> STOPVD maps p->b, t->d, k->g in parallel, intervocalically
        let rules = ["class STOPV = p t k", "class STOPVD = b d g", "class V = a e i o u", "STOPV > STOPVD / V _ V"];
        assert_eq!(sca("apataka", &rules), "abadaga");
    }

    #[test]
    fn ligurian_solar_to_heart() {
        let cases = [
            ("ligā", "liɣō"), ("dīwā", "nīvō"), ("bazdā", "mazdō"), ("Naktā", "Naktō"),
            ("orpā", "orpō"), ("asā", "azō"), ("apā", "avō"), ("moisā", "moizō"),
            ("kósmā", "gozmō"), ("sālā", "hōlō"), ("wōkā", "vūɣō"), ("zāwā", "zōvō"),
            ("weidā", "veiðō"),
        ];
        for (s, h) in cases {
            assert_eq!(evolve(s).unwrap(), h, "evolving {}", s);
        }
    }

    #[test]
    fn ligurian_breaking_and_nasalization() {
        let cases = [
            ("zelā", "zealō"), ("serdā", "heardō"), ("sanā", "haonō"), ("dázā", "naozō"),
            ("zerā", "zearō"), ("menā", "meanō"), ("berzā", "mearzō"),
            ("bendā", "mẽdō"), ("genton", "gẽtõ"), ("kamtón", "gãtõ"), ("mēlon", "mīlõ"),
            ("ostón", "ostõ"), ("aiwon", "aivõ"),
            ("nūn", "nū\u{0303}"), ("ōn", "ū\u{0303}"),
        ];
        for (s, h) in cases {
            assert_eq!(evolve(s).unwrap(), h, "evolving {}", s);
        }
    }

    #[test]
    fn ligurian_file_parses_and_expands_classes() {
        let rules = parse_sca_file(LIGURIAN_SCA).unwrap();
        assert!(rules.len() > 100, "expected class expansion, got {}", rules.len());
    }

    #[test]
    fn stress_and_cluster_conditioned_rules() {
        // Stressed-vowel breaking fires only in an OPEN syllable (stressed V, one C, then V),
        // so it is conditioned on both stress and the following cluster.
        let break_rules = "class V = a e i o u\nclass C = p t k s n r l\ná > a o / _ C V";
        assert_eq!(run_sca("kása", &[break_rules.to_string()]).unwrap(), "kaosa"); // open: breaks
        assert_eq!(run_sca("kásta", &[break_rules.to_string()]).unwrap(), "kásta"); // closed: no break
        // a cluster-conditioned onset deletion
        assert_eq!(
            run_sca("spata", &["class C = p t k\ns > / # _ C".to_string()]).unwrap(),
            "pata"
        );
    }
}
