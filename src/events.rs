//! events — EVENT-AWARE, RETURN-SUPERVISED news scoring: the step past plain sentiment.
//!
//! The research verdict has moved on from monolithic polarity. Two findings drive this
//! module's design:
//!
//! 1. **Event-conditioned signals beat plain polarity.** A single sentiment number treats
//!    "ETF outflows deepen" and "exchange hacked" and "Fed signals cuts" as the same kind of
//!    thing, but different narrative types have different return profiles AND different decay
//!    rates (an exchange hack reprices risk for days; retail buzz is stale in hours). The
//!    event-factor literature (LLM-tagged tweet factors extending SESTM, arXiv 2508.07408)
//!    shows event-conditioned sentiment factors carry distinct, more interpretable predictive
//!    profiles than one polarity score. Within the zero-dependency rule, the tagger here is a
//!    white-box multi-label pattern classifier over a finance/crypto event taxonomy; each
//!    event class carries an IMPACT multiplier (how much a story of this kind should count)
//!    and a HALF-LIFE (how fast it should stop counting).
//!
//! 2. **Supervision by returns beats supervision by human labels.** Ke, Kelly & Xiu's SESTM
//!    ("Predicting Returns with Text Data", NBER w26186) learns the lexicon FROM price
//!    responses: screen terms whose appearance coincides with same-sign next-day returns,
//!    estimate positive/negative topic weights from the up/down-day corpora, then score an
//!    article by penalized maximum likelihood. It is deliberately "white box", runs on a
//!    laptop, and beat the leading commercial vendor (RavenPack) head-to-head. SESTM-lite
//!    below is that three-step procedure, trained on the news wire this system accumulates,
//!    aligned with the market's own price series — the model literally learns what words
//!    have moved THIS market. HONEST FRAMING: it only participates once it has enough dated
//!    items to train on (>= MIN_TRAIN), and every report says whether it did.
//!
//! The per-item score fuses: the trained classifier + LM lexicon (`sentiment::polarity_fused`,
//! the previous engine, still the floor), the SESTM score when trained, and the event
//! conditioning (impact + half-life feed the aggregation WEIGHT, not the polarity — a big
//! story counts for more and decays on its own clock, but its direction is what the text
//! says). `newswire::score_wire` does the weighted fusion, with the final ratio computed on
//! Loom (lib/sentiment.lat `wagg`), keeping the language in the loop.

/// One event class: tag, aggregation-impact multiplier, half-life in days, patterns.
/// Patterns are lowercase substrings; a match anywhere in the lowercased text tags the item.
pub struct EventClass {
    pub tag: &'static str,
    pub impact: f64,
    pub half_life_days: f64,
    pub patterns: &'static [&'static str],
}

/// The taxonomy. Impact and half-life encode the event-factor finding that narrative types
/// differ in both magnitude and persistence. Values are documented judgments, not fits:
/// security incidents and fund flows hit hardest; policy/regulation persists longest;
/// leverage/liquidation stories are violent but stale by the next session; retail buzz is
/// discounted (impact 0.6) and decays in hours.
pub const EVENTS: &[EventClass] = &[
    EventClass { tag: "etf-flow", impact: 1.30, half_life_days: 2.0,
        patterns: &["etf", "inflow", "outflow", "fund flow", "ibit", "gbtc", "spot fund"] },
    EventClass { tag: "regulation", impact: 1.20, half_life_days: 7.0,
        patterns: &["sec ", "regulat", "lawsuit", "court", " ban ", "banned", "approval",
                    "approve", "license", "mica", "cftc", "legal", "compliance", "sanction"] },
    EventClass { tag: "hack", impact: 1.40, half_life_days: 5.0,
        patterns: &["hack", "exploit", "breach", "stolen", "theft", "drained", "vulnerabilit"] },
    EventClass { tag: "macro-rates", impact: 1.10, half_life_days: 3.0,
        patterns: &["fed ", "fomc", "rate cut", "rate hike", "inflation", "cpi", "payroll",
                    "treasury", "yields", "powell", "recession", "jobs report", "dovish", "hawkish"] },
    EventClass { tag: "adoption", impact: 1.15, half_life_days: 7.0,
        patterns: &["adopt", "institutional", "blackrock", "fidelity", "acquires", "accumul",
                    "strategy buys", "partnership", "integrat", "payment", "reserve"] },
    EventClass { tag: "tech", impact: 0.90, half_life_days: 7.0,
        patterns: &["upgrade", "hard fork", "halving", "protocol", "mainnet", "lightning", "scaling"] },
    EventClass { tag: "market-structure", impact: 1.20, half_life_days: 1.0,
        patterns: &["liquidat", "leverage", "margin call", "futures", "open interest",
                    "short squeeze", "funding rate", "whale"] },
    EventClass { tag: "geopolitics", impact: 1.00, half_life_days: 2.0,
        patterns: &["war", "military", "strike", "geopolit", "conflict", "ceasefire", "drone"] },
    EventClass { tag: "retail-buzz", impact: 0.60, half_life_days: 0.5,
        patterns: &["moon", "hodl", "fomo", "diamond hands", "pump", "meme", "lambo", "ath "] },
];

/// The event conditioning for one text: matched tags, the aggregation impact (max of matched
/// classes; 1.0 when none), and the half-life (max of matched; `default_hl` when none —
/// callers pass the kind's convention: 3 days press, 0.5 days social chatter).
pub fn classify(text: &str, default_hl: f64) -> (Vec<&'static str>, f64, f64) {
    let low = format!(" {} ", text.to_lowercase());
    let mut tags = Vec::new();
    let mut impact: Option<f64> = None; // max over MATCHED classes — so a pure retail-buzz
    let mut hl = default_hl;            // item is genuinely discounted (0.6), while buzz
    for ev in EVENTS {                  // mixed with a hack still amplifies (1.4)
        if ev.patterns.iter().any(|p| low.contains(p)) {
            tags.push(ev.tag);
            impact = Some(impact.map_or(ev.impact, |i: f64| i.max(ev.impact)));
            hl = hl.max(ev.half_life_days);
        }
    }
    (tags, impact.unwrap_or(1.0), hl)
}

// ===========================================================================
// SESTM-lite — Sentiment Extraction via Screening and Topic Modeling
// (Ke–Kelly–Xiu 2019), the return-supervised replacement for hand lexicons,
// trained on the accumulated news wire against the market's price series.
// ===========================================================================

/// Minimum dated training items before the model participates in live scoring.
pub const MIN_TRAIN: usize = 60;
/// Screening: a term must occur in at least KAPPA items…
const KAPPA: usize = 3;
/// …and its up-day frequency must sit at least ALPHA away from 1/2.
const ALPHA: f64 = 0.15;
/// The penalized-likelihood prior weight toward p = 1/2.
const LAMBDA: f64 = 0.2;

/// A trained SESTM model: screened sentiment-charged terms with their positive- and
/// negative-topic weights, plus provenance for honest reporting.
pub struct Sestm {
    pub terms: Vec<(String, f64, f64)>, // (term, O+, O-), sorted by term
    pub trained_on: usize,              // labeled items used
    pub span: (String, String),         // first/last training date
}

/// The model tokenizer: alphabetic unigrams, lowercased, stopword-filtered — the same
/// discipline as the trained classifier (negators kept), unigrams only (wire items are short
/// and the corpus starts small; bigrams can join once the wire has history).
pub fn terms(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "on", "in", "of",
        "to", "as", "and", "or", "for", "at", "by", "with", "after", "before", "has", "have",
        "had", "its", "it", "this", "that", "these", "those", "we", "our", "their", "over",
        "under", "from",
    ];
    text.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|t| t.len() > 1)
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| !STOP.contains(&t.as_str()))
        .collect()
}

impl Sestm {
    /// Train on (date, headline) items against a (date, close x100) daily series: each item
    /// is labeled by the SIGN of the next trading day's return on its date (items whose date
    /// has no d -> d+1 pair in the series are skipped). Steps: (1) screen terms by up-day
    /// frequency, (2) estimate O+ / O- from the up/down corpora with Laplace smoothing.
    pub fn train(items: &[(String, String)], series: &[(String, i64)]) -> Result<Sestm, String> {
        // date -> next-day return sign (+1 up, -1 down; zero-change days are skipped)
        let mut sign: std::collections::HashMap<&str, i32> = std::collections::HashMap::new();
        for w in series.windows(2) {
            let d = (w[1].1 - w[0].1).signum();
            if d != 0 {
                sign.insert(w[0].0.as_str(), d as i32);
            }
        }
        let mut labeled = 0usize;
        let (mut lo, mut hi) = (String::new(), String::new());
        // per-term counts in up-labeled vs down-labeled items, and item counts per term
        let mut cnt: std::collections::HashMap<String, (usize, usize, usize)> =
            std::collections::HashMap::new(); // (in-up occurrences, in-down, item count)
        let (mut up_total, mut down_total) = (0usize, 0usize);
        for (date, headline) in items {
            let s = match sign.get(date.as_str()) {
                Some(s) => *s,
                None => continue,
            };
            labeled += 1;
            if lo.is_empty() || date < &lo { lo = date.clone(); }
            if hi.is_empty() || date > &hi { hi = date.clone(); }
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for t in terms(headline) {
                let e = cnt.entry(t.clone()).or_insert((0, 0, 0));
                if s > 0 { e.0 += 1; up_total += 1; } else { e.1 += 1; down_total += 1; }
                if seen.insert(t) { e.2 += 1; }
            }
        }
        if labeled < MIN_TRAIN {
            return Err(format!(
                "not enough labeled items to train: {} dated items matched the price series (need >= {}) — keep the wire running (`latte news fetch`) and retrain later",
                labeled, MIN_TRAIN
            ));
        }
        // screening: frequent enough, and up-fraction far enough from 1/2
        let mut screened: Vec<(String, usize, usize)> = cnt
            .into_iter()
            .filter(|(_, (u, d, n))| {
                let tot = u + d;
                *n >= KAPPA && tot > 0 && ((*u as f64 / tot as f64) - 0.5).abs() >= ALPHA
            })
            .map(|(t, (u, d, _))| (t, u, d))
            .collect();
        if screened.is_empty() {
            return Err("screening kept no terms — the wire and the series do not align yet".into());
        }
        screened.sort_by(|a, b| a.0.cmp(&b.0));
        // topic weights with Laplace smoothing, normalized over the screened vocabulary
        let v = screened.len() as f64;
        let (su, sd) = (up_total as f64 + v * 0.5, down_total as f64 + v * 0.5);
        let terms = screened
            .into_iter()
            .map(|(t, u, d)| (t, (u as f64 + 0.5) / su, (d as f64 + 0.5) / sd))
            .collect();
        Ok(Sestm { terms, trained_on: labeled, span: (lo, hi) })
    }

    /// Score a text: maximize the penalized log-likelihood over p in (0,1) —
    /// sum_j c_j log(p O+_j + (1-p) O-_j) + LAMBDA log(p (1-p)) — and map to a polarity
    /// 2p - 1 in [-1, 1]. None when the text contains no screened (sentiment-charged) term:
    /// SESTM abstains rather than guessing, and the caller falls back to the fused score.
    pub fn score(&self, text: &str) -> Option<f64> {
        let mut c: Vec<(usize, usize)> = Vec::new(); // (term index, count)
        for t in terms(text) {
            if let Ok(i) = self.terms.binary_search_by(|(w, _, _)| w.as_str().cmp(t.as_str())) {
                match c.iter_mut().find(|(j, _)| *j == i) {
                    Some((_, n)) => *n += 1,
                    None => c.push((i, 1)),
                }
            }
        }
        if c.is_empty() {
            return None;
        }
        let ll = |p: f64| -> f64 {
            let mut s = LAMBDA * (p * (1.0 - p)).ln();
            for (i, n) in &c {
                let (_, op, om) = &self.terms[*i];
                s += *n as f64 * (p * op + (1.0 - p) * om).max(1e-300).ln();
            }
            s
        };
        // the objective is smooth and unimodal in p on (0,1): a fine grid is exact enough
        let (mut best_p, mut best) = (0.5, f64::NEG_INFINITY);
        for k in 1..100 {
            let p = k as f64 / 100.0;
            let v = ll(p);
            if v > best {
                best = v;
                best_p = p;
            }
        }
        Some(2.0 * best_p - 1.0)
    }

    /// Persist to a TSV: a `#meta` first line, then term rows. Weights stored x1e9.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut s = format!("#meta\t{}\t{}\t{}\n", self.trained_on, self.span.0, self.span.1);
        for (t, op, om) in &self.terms {
            s.push_str(&format!("{}\t{:.0}\t{:.0}\n", t, op * 1e9, om * 1e9));
        }
        std::fs::write(path, s)
    }

    /// Load a persisted model; None when the file is absent or malformed.
    pub fn load(path: &std::path::Path) -> Option<Sestm> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut lines = text.lines();
        let meta: Vec<&str> = lines.next()?.split('\t').collect();
        if meta.len() != 4 || meta[0] != "#meta" {
            return None;
        }
        let trained_on = meta[1].parse().ok()?;
        let span = (meta[2].to_string(), meta[3].to_string());
        let mut terms = Vec::new();
        for l in lines {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() == 3 {
                let (op, om) = (f[1].parse::<f64>().ok()?, f[2].parse::<f64>().ok()?);
                terms.push((f[0].to_string(), op / 1e9, om / 1e9));
            }
        }
        if terms.is_empty() {
            return None;
        }
        terms.sort_by(|a, b| a.0.cmp(&b.0));
        Some(Sestm { terms, trained_on, span })
    }
}

/// Where a market's SESTM model lives (beside the market cache, same override).
pub fn model_path(market: &str) -> std::path::PathBuf {
    crate::newswire::news_cache_dir().join(format!("sestm-{}.tsv", market))
}

/// The per-item polarity the wire uses: the fused classifier+lexicon score, blended half-and-
/// half with SESTM when a trained model speaks for this text (the return-supervised view and
/// the label-supervised view each carry half — neither alone decides). `bond` swaps the base
/// for the hawk/dove bond scorer; SESTM (trained on the risk asset's own returns) does not
/// apply to the bond axis.
pub fn item_polarity(text: &str, model: Option<&Sestm>, bond: bool) -> f64 {
    if bond {
        return crate::sentiment::bond_polarity(text);
    }
    let base = crate::sentiment::polarity_fused(text);
    match model.and_then(|m| if m.trained_on >= MIN_TRAIN { m.score(text) } else { None }) {
        Some(s) => 0.5 * s + 0.5 * base,
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_tags_the_taxonomy() {
        let (tags, impact, hl) = classify("Spot bitcoin ETF outflows deepen as fund flows reverse", 3.0);
        assert!(tags.contains(&"etf-flow"));
        assert!((impact - 1.30).abs() < 1e-9);
        assert!((hl - 3.0).abs() < 1e-9, "etf half-life (2d) does not shorten the press default (3d)");
        let (tags, impact, hl) = classify("Exchange hacked, $40M stolen in security breach", 0.5);
        assert!(tags.contains(&"hack"));
        assert!((impact - 1.40).abs() < 1e-9);
        assert!((hl - 5.0).abs() < 1e-9, "a hack outlives social chatter's half-day default");
    }

    #[test]
    fn classify_untagged_text_is_neutral_conditioning() {
        let (tags, impact, hl) = classify("the committee will publish the schedule", 3.0);
        assert!(tags.is_empty());
        assert_eq!(impact, 1.0);
        assert_eq!(hl, 3.0);
    }

    #[test]
    fn multiple_events_take_the_max_impact_and_half_life() {
        let (tags, impact, hl) =
            classify("Regulators approve ETF after exchange hack; SEC lawsuit dropped", 3.0);
        assert!(tags.len() >= 3);
        assert!((impact - 1.40).abs() < 1e-9, "hack dominates impact");
        assert!((hl - 7.0).abs() < 1e-9, "regulation dominates persistence");
    }

    fn synth_corpus() -> (Vec<(String, String)>, Vec<(String, i64)>) {
        // 40 alternating days: rally-headline days precede up days, selloff days precede down
        let mut series = Vec::new();
        let mut items = Vec::new();
        let mut px = 100_000i64;
        for i in 0..80 {
            let d = format!("2026-03-{:02}", (i % 28) + 1);
            let d = format!("{}{}", if i < 28 { "" } else { "x" }, d); // unique keys, ordered
            series.push((d.clone(), px));
            let up = i % 2 == 0;
            px += if up { 500 } else { -500 };
            items.push((
                d,
                if up {
                    "bullish surge accumulation strong demand today".to_string()
                } else {
                    "bearish capitulation weak selling pressure today".to_string()
                },
            ));
        }
        (items, series)
    }

    #[test]
    fn sestm_learns_return_charged_terms_and_scores_them() {
        let (items, series) = synth_corpus();
        let m = Sestm::train(&items, &series).expect("training succeeds on the synthetic corpus");
        assert!(m.trained_on >= MIN_TRAIN);
        let bull = m.score("surge in demand, strong accumulation").expect("charged terms present");
        let bear = m.score("capitulation and heavy selling, weak market").expect("charged terms present");
        assert!(bull > 0.3, "bull text must score clearly positive: {}", bull);
        assert!(bear < -0.3, "bear text must score clearly negative: {}", bear);
        assert!(m.score("the committee publishes the schedule").is_none(), "SESTM abstains off-vocabulary");
    }

    #[test]
    fn sestm_roundtrips_through_the_tsv() {
        let (items, series) = synth_corpus();
        let m = Sestm::train(&items, &series).unwrap();
        let dir = std::env::temp_dir().join(format!("orpheus-sestm-test-{}", std::process::id()));
        let path = dir.join("sestm-btc.tsv");
        m.save(&path).unwrap();
        let l = Sestm::load(&path).expect("loads back");
        assert_eq!(l.trained_on, m.trained_on);
        assert_eq!(l.terms.len(), m.terms.len());
        let (a, b) = (m.score("strong surge").unwrap(), l.score("strong surge").unwrap());
        assert!((a - b).abs() < 0.02, "persisted model scores the same");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sestm_is_honest_about_small_corpora() {
        let items = vec![("2026-03-01".to_string(), "rally".to_string())];
        let series = vec![("2026-03-01".to_string(), 1000i64), ("2026-03-02".to_string(), 1100i64)];
        assert!(Sestm::train(&items, &series).is_err(), "refuses to train on too little");
    }

    #[test]
    fn item_polarity_blends_when_trained_and_falls_back_when_not() {
        let (items, series) = synth_corpus();
        let m = Sestm::train(&items, &series).unwrap();
        let with = item_polarity("strong surge in demand", Some(&m), false);
        let without = item_polarity("strong surge in demand", None, false);
        assert!(with > 0.0 && without > 0.0);
        // off-vocabulary: the blend must equal the fused base exactly (SESTM abstained)
        let t = "quarterly schedule published";
        assert_eq!(item_polarity(t, Some(&m), false), crate::sentiment::polarity_fused(t));
    }
}
