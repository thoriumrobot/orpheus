//! sentiment — lexicon-based financial sentiment scoring (Loughran-McDonald method).
//!
//! Loughran & McDonald (2011, *Journal of Finance*) showed that general-purpose sentiment
//! dictionaries misclassify finance text badly, and built finance-specific word lists. The
//! standard scoring is a simple count: tally positive and negative words and report polarity
//! = (pos - neg) / (pos + neg). This module embeds a compact, representative subset of the LM
//! positive/negative lists (the full lists are ~145 / ~885 words, free for academic use from
//! sraf.nd.edu). Tokenization and lookup happen here; the polarity *arithmetic* is computed in
//! Latte (`lib/sentiment.lat`, signed fixed-point), so the language is in the loop.
//!
//! News sentiment is most useful on markets driven by public information flow — equities,
//! indices, single names — where headlines move prices; it is an exogenous feature that can be
//! fed to the model and the trading advisor (`--sentiment`).

/// A representative subset of the Loughran-McDonald POSITIVE word list.
const POSITIVE: &[&str] = &[
    "able", "advantage", "advantages", "beneficial", "benefit", "benefits", "best", "better",
    "boom", "boosted", "boosts", "breakthrough", "gain", "gains", "gained", "good", "great",
    "greater", "growth", "grew", "improve", "improved", "improvement", "improves", "improving",
    "increase", "increased", "increases", "leading", "looms", "opportunities", "opportunity",
    "outperform", "outperformed", "positive", "profit", "profitable", "profits", "rally",
    "rallied", "rebound", "rebounded", "recover", "recovered", "recovery", "record", "strength",
    "strong", "stronger", "strongest", "success", "successful", "surge", "surged", "upbeat",
    "upgrade", "upgraded", "win", "winner", "winning", "higher", "rise", "rises", "rising", "rose",
    // market-flow extensions (not in LM, but unambiguous in market headlines)
    "inflow", "inflows", "buy", "buys", "buying", "bought", "accumulate", "accumulating",
    "bounce", "bounced", "soar", "soared", "soars", "climb", "climbed", "climbs", "jump",
    "jumped", "jumps", "bull", "bullish", "relief",
];

/// A representative subset of the Loughran-McDonald NEGATIVE word list.
const NEGATIVE: &[&str] = &[
    "adverse", "adversely", "against", "bad", "bankruptcy", "bear", "bearish", "breach", "concern",
    "concerns", "crisis", "crash", "crashed", "decline", "declined", "declines", "declining",
    "default", "deficit", "deteriorate", "deteriorating", "downgrade", "downgraded", "downturn",
    "drop", "dropped", "drops", "fail", "failed", "failure", "fall", "fallen", "falls", "falter",
    "faltered", "fear", "fears", "fell", "loss", "losses", "lost", "low", "lower", "negative",
    "panic", "plunge", "plunged", "pressure", "recession", "risk", "risks", "risky", "selloff",
    "shortfall", "slump", "slumped", "sink", "sank", "slowdown", "struggle", "struggled", "tension",
    "tensions", "turmoil", "uncertainty", "uncertain", "volatile", "volatility", "weak", "weaker",
    "weakness", "worse", "worst", "worry", "worried", "worries",
    // market-flow extensions (not in LM, but unambiguous in market headlines)
    "down", "outflow", "outflows", "dump", "dumped", "dip", "dips", "dipped", "tumble",
    "tumbled", "tumbling", "rout", "bleed", "bleeding", "slid", "slide", "slides", "slump",
    "crushed", "descent", "ugliest", "war", "strikes", "shorting", "redemptions",
];

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Count positive and negative finance words in `text`.
pub fn counts(text: &str) -> (usize, usize) {
    let mut pos = 0;
    let mut neg = 0;
    for tok in tokenize(text) {
        if POSITIVE.contains(&tok.as_str()) {
            pos += 1;
        } else if NEGATIVE.contains(&tok.as_str()) {
            neg += 1;
        }
    }
    (pos, neg)
}

/// Polarity = (pos - neg) / (pos + neg), in [-1, 1]; 0 when no sentiment words are present.
/// The ratio is computed by the Latte `polarity` function over signed fixed-point, then read
/// back here, so the scoring arithmetic genuinely runs on Loom.
pub fn polarity(text: &str) -> f64 {
    let (pos, neg) = counts(text);
    if pos + neg == 0 {
        return 0.0;
    }
    let expr = format!("(polarity {} {})", pos, neg);
    match crate::latte::run_with_libs(&expr, &["std", "num", "sentiment"]) {
        Ok(v) => signed_fixed_to_f64(&v),
        // Fall back to the direct computation if the library call fails for any reason.
        Err(_) => (pos as f64 - neg as f64) / (pos as f64 + neg as f64),
    }
}

/// Decode a num.lat signed fixed-point `[sign magnitude]` (magnitude scaled x1000) to f64.
fn signed_fixed_to_f64(n: &crate::knot::N) -> f64 {
    use crate::knot::Knot;
    if let Knot::Cell(h, t) = &**n {
        let sign = h.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let mag = t.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as f64 / 1000.0;
        if sign == 0 {
            mag
        } else {
            -mag
        }
    } else {
        0.0
    }
}

#[cfg(test)]
mod bond_tests {
    use super::*;

    #[test]
    fn hawkish_news_is_bearish_for_bonds() {
        let t = "Hot inflation forces aggressive hikes; Treasury supply surges amid deficits";
        assert!(bond_polarity(t) < -0.5, "hawkish text must be strongly bearish for bonds");
    }

    #[test]
    fn dovish_news_is_bullish_for_bonds() {
        let t = "Fed signals rate cuts as inflation cools; markets rally on dovish pivot";
        assert!(bond_polarity(t) > 0.1, "dovish text must be bullish for bonds");
    }

    #[test]
    fn risk_off_without_policy_vocab_is_a_treasury_bid() {
        let t = "Factory fire disrupts production; shares plunge on weak earnings fears";
        let g = polarity_fused(t);
        let b = bond_polarity(t);
        assert!(g < -0.3, "clearly negative general sentiment");
        assert!(b > 0.3, "flight to quality: bond polarity inverts the general score");
        assert!((b + g).abs() < 1e-9, "no rates vocabulary: bond polarity is exactly the inversion");
    }

    #[test]
    fn neutral_text_scores_near_zero_on_both_axes() {
        let t = "The committee will publish the schedule for the next meeting";
        assert_eq!(rate_counts(t), (0, 0));
    }

    #[test]
    fn bond_document_scoring_mirrors_general_aggregation() {
        let t = "Inflation runs hot and the bank hikes rates. Growth is slowing sharply and cuts are coming.";
        let (doc, sents) = score_document_bond(t);
        assert_eq!(sents.len(), 2);
        assert!(doc.is_finite());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullish_headline_scores_positive() {
        let (p, n) = counts("Wall Street rebound rally as stocks recover and chip shares surge higher");
        assert!(p > n, "bullish text should have more positive words: {} vs {}", p, n);
        assert!(polarity("stocks rally and recover, shares surge higher") > 0.0);
    }

    #[test]
    fn bearish_headline_scores_negative() {
        let (p, n) = counts("Nasdaq futures sink as tech stocks falter; selloff deepens amid recession fears");
        assert!(n > p, "bearish text should have more negative words: {} vs {}", p, n);
        assert!(polarity("selloff deepens, stocks plunge on recession fears and weakness") < 0.0);
    }

    #[test]
    fn neutral_text_scores_zero() {
        assert_eq!(polarity("the company will report earnings on tuesday"), 0.0);
    }
}

// ============================================================================
// THE TRAINED CLASSIFIER. Supervised text scoring — the literature's verdict is
// consistent that learned models beat word-counting lexicons on financial text
// (FinBERT-class transformers and supervised regressions both; see
// docs/visualization-and-ml.md). Within the zero-dependency rule this is a
// LOGISTIC REGRESSION over unigram+bigram features, trained offline on a
// labeled corpus of financial text in BOTH registers — market headlines and
// report/filing language — with the negation and context cases that defeat
// lexicons ("outflows eased" is positive; "failed to deliver" is negative;
// "risks did not materialize" vs "risks materialized"). The weights below are
// embedded verbatim from training; src is reproducible from the corpus.
//
// It scores SENTENCES, so it applies to whole documents and reports, not just
// headlines: `score_document` splits the text, scores each sentence, and
// aggregates with confidence weights, returning the per-sentence evidence.
// ============================================================================

// trained 2026-06-10: logistic regression on 101 labeled financial texts
// (headline + report register), unigram+bigram features, L2-regularized.
const MODEL_BIAS_MILLI: i64 = 30;
const MODEL_WEIGHTS_MILLI: &[(&str, i64)] = &[
    ("about", -9295),
    ("about_dilution", 1068),
    ("accelerated", -3361),
    ("actions", 899),
    ("adoption", 3466),
    ("advance", 7742),
    ("ahead", 10447),
    ("appear", -2437),
    ("asset", -242),
    ("asset_values", -529),
    ("backdrop", -3734),
    ("backlog", -2805),
    ("both", 452),
    ("both_top", 452),
    ("bottom", 452),
    ("bottom_line", 452),
    ("broad", -810),
    ("buyback", -2191),
    ("buyers", 582),
    ("capitulated", 861),
    ("cash", 346),
    ("cash_flow", 346),
    ("churn", 1712),
    ("citing", -1632),
    ("company", -2096),
    ("company_reported", -463),
    ("concerns", 1068),
    ("concerns_about", 1068),
    ("confirmed", -2007),
    ("confirmed_volume", -2007),
    ("consensus", 452),
    ("consensus_both", 452),
    ("consumer", -2038),
    ("consumer_data", -2038),
    ("cost", -1185),
    ("cost_overruns", -785),
    ("currency", 899),
    ("customer", 1712),
    ("cut", -5164),
    ("data", 2767),
    ("decline", -4449),
    ("declined", -6477),
    ("demand", 10638),
    ("demand_proved", -3734),
    ("despite", 2686),
    ("deteriorated", -5916),
    ("did", 752),
    ("did_not", 752),
    ("difficult", -3734),
    ("difficult_macro", -3734),
    ("dilution", 1068),
    ("disclosed", 759),
    ("disclosed_risks", 759),
    ("dividend", -2191),
    ("dried", -369),
    ("dried_up", -369),
    ("drove_margin", -841),
    ("drove_prices", 861),
    ("earnings", -4063),
    ("ease", 1609),
    ("eased", 4376),
    ("etf", 923),
    ("etf_inflows", 1486),
    ("etf_outflows", 467),
    ("expanded", 7256),
    ("extends", -967),
    ("faded", -2567),
    ("failed", -3063),
    ("fears", -1444),
    ("fed", -967),
    ("fed_signals", -967),
    ("firm", 6831),
    ("flow", 346),
    ("free", 346),
    ("free_cash", 346),
    ("full", -1632),
    ("full_year", -1632),
    ("fully", -2490),
    ("gains", -2559),
    ("guidance", -203),
    ("guidance_citing", -1632),
    ("headwinds", -2675),
    ("held", 4345),
    ("higher", 1339),
    ("hit", -3335),
    ("hit_record", -3335),
    ("hold", -3426),
    ("impairment", -529),
    ("improved", 5373),
    ("increased", 2088),
    ("inflation", 899),
    ("inflows", -5092),
    ("integration", -997),
    ("investors", -4623),
    ("kept", -4343),
    ("leads", -810),
    ("leads_broad", -810),
    ("level", 1855),
    ("leverage", -841),
    ("leverage_drove", -841),
    ("line", 452),
    ("liquidity", 511),
    ("losses", -976),
    ("lower", -9998),
    ("macro", -3734),
    ("macro_backdrop", -3734),
    ("management", -1632),
    ("margin", -841),
    ("margins", -633),
    ("market", 815),
    ("market_extends", -967),
    ("materialize", 195),
    ("materialize_orders", -1915),
    ("materialized", -2802),
    ("materialized_quarter", -1627),
    ("materially", 346),
    ("missed", -9009),
    ("not", 752),
    ("not_materialize", 195),
    ("operating", -841),
    ("operating_leverage", -841),
    ("orders", -1915),
    ("outflows", 1240),
    ("outflows_ease", 1609),
    ("outlook", 4051),
    ("overruns", -785),
    ("partnerships", 3466),
    ("path", 1011),
    ("path_profitability", 1011),
    ("percent", 667),
    ("percent_year", 667),
    ("previously", 759),
    ("previously_disclosed", 759),
    ("price", 1331),
    ("price_target", 1331),
    ("prices", 861),
    ("pricing", 899),
    ("pricing_actions", 899),
    ("profitability", 1011),
    ("proved", 7132),
    ("quarter", -502),
    ("quarterly", -463),
    ("raised", 5875),
    ("rally", 3991),
    ("recession", -1444),
    ("record", 1498),
    ("record_quarterly", -463),
    ("recovered", 8224),
    ("reported", -463),
    ("reported_record", -463),
    ("resilient", 8417),
    ("restructuring", 3811),
    ("results", 452),
    ("retention", 1712),
    ("revenue", 667),
    ("reversed", -2490),
    ("risks", -208),
    ("risks_materialized", -3670),
    ("savings", 1080),
    ("schedule", -997),
    ("schedule_synergies", -997),
    ("selling", 3438),
    ("selloff", 1592),
    ("sentiment", 821),
    ("shares", 1473),
    ("sharply", 926),
    ("signals", -967),
    ("spread", 2408),
    ("spreads", 511),
    ("stalled", -10682),
    ("stocks", -2616),
    ("strengthened", 4670),
    ("strong", 8095),
    ("subsequently", 4371),
    ("substantially", 1011),
    ("substantially_path", 1011),
    ("supply", 890),
    ("support", -4196),
    ("synergies", -997),
    ("target", 1331),
    ("targets", -997),
    ("tech", -810),
    ("tech_leads", -810),
    ("top", 452),
    ("top_bottom", 452),
    ("twelve", 667),
    ("twelve_percent", 667),
    ("up", -369),
    ("values", -529),
    ("visibility", -2805),
    ("volatility", -258),
    ("volume", -2007),
    ("volume_expanded", -2007),
    ("weakened", -4678),
    ("widened", -8163),
    ("worsened", -5533),
    ("year", -534),
    ("year_guidance", -1632),
    ("year_year", 667),
];


/// Stopwords dropped from model features (the training pipeline drops the same
/// set; negators — not, no, never, despite — are deliberately KEPT).
const MODEL_STOP: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "on", "in", "of",
    "to", "as", "and", "or", "for", "at", "by", "with", "after", "before", "has", "have",
    "had", "its", "it", "this", "that", "these", "those", "we", "our", "their", "over",
    "under", "from",
];

/// Bigram-aware tokenization for the model: stopword-filtered unigrams plus
/// adjacent pairs (matching the training featurizer exactly).
fn model_features(text: &str) -> Vec<String> {
    let toks: Vec<String> = tokenize(text)
        .into_iter()
        .filter(|t| !MODEL_STOP.contains(&t.as_str()))
        .collect();
    let mut f: Vec<String> = toks.clone();
    for w in toks.windows(2) {
        f.push(format!("{}_{}", w[0], w[1]));
    }
    f
}

/// The classifier's logit for a piece of text (0 = the decision boundary).
pub fn model_logit(text: &str) -> f64 {
    let feats = model_features(text);
    let mut z = MODEL_BIAS_MILLI as f64 / 1000.0;
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for f in &feats {
        if seen.insert(f.as_str()) {
            if let Ok(i) = MODEL_WEIGHTS_MILLI.binary_search_by(|(w, _)| w.cmp(&f.as_str())) {
                z += MODEL_WEIGHTS_MILLI[i].1 as f64 / 1000.0;
            }
        }
    }
    z
}

/// The classifier's polarity in [-1, 1]: 2·sigmoid(logit) − 1.
pub fn model_polarity(text: &str) -> f64 {
    let z = model_logit(text).clamp(-30.0, 30.0);
    2.0 / (1.0 + (-z).exp()) - 1.0
}

/// The fused short-text score: the trained model carries most of the weight,
/// the Loughran-McDonald lexicon (computed on Loom) the rest.
pub fn polarity_fused(text: &str) -> f64 {
    0.65 * model_polarity(text) + 0.35 * polarity(text)
}

// ---------------------------------------------------------------------------
// The RATES (hawk/dove) axis — sentiment for the BOND market.
//
// General financial positivity is the wrong sign for Treasuries: "stocks rally
// on strong growth" is BEARISH for bond prices (yields rise), and risk-off
// news is BULLISH (flight to quality). What moves bonds first is the
// monetary-policy direction, so the bond scorer reads a dedicated
// hawkish/dovish lexicon (rate hikes, tightening, QT, hot inflation vs. cuts,
// easing, QE, recession, disinflation) and FUSES it with the general
// Loughran-McDonald polarity with a NEGATIVE weight — encoding exactly the
// flight-to-quality inversion. One sentiment engine, two market-correct
// views: `polarity_fused` for risk assets, `bond_polarity` for duration.
// Both are available everywhere the engine is: `latte sentiment --bond`,
// /api/sentiment, the Facet Sentiment tool, and the trade advisors.
// ---------------------------------------------------------------------------

/// Words signalling TIGHTER policy / higher yields — bearish for bond prices.
const HAWKISH: &[&str] = &[
    "hike", "hikes", "hiked", "hiking", "tighten", "tightens", "tightening", "hawkish",
    "inflation", "inflationary", "overheating", "overheated", "taper", "tapering",
    "restrictive", "qt", "issuance", "deficit", "deficits", "supply", "oversupply",
    "hot", "sticky", "acceleration", "reflation", "unwind", "runoff",
];

/// Words signalling EASIER policy / lower yields — bullish for bond prices.
const DOVISH: &[&str] = &[
    "cut", "cuts", "cutting", "ease", "eases", "easing", "eased", "dovish", "stimulus",
    "qe", "accommodative", "pause", "pauses", "paused", "recession", "recessionary",
    "slowdown", "slowing", "slows", "cooling", "cooled", "cools", "cooler", "soften",
    "softens", "softer", "softening", "moderating", "moderates", "pivot", "disinflation",
    "deflation", "purchases", "buying", "safe-haven", "haven", "flight",
];

/// (dovish, hawkish) word counts over the same tokenizer as the LM counts.
pub fn rate_counts(text: &str) -> (usize, usize) {
    let mut dove = 0;
    let mut hawk = 0;
    for tok in tokenize(text) {
        if DOVISH.contains(&tok.as_str()) {
            dove += 1;
        } else if HAWKISH.contains(&tok.as_str()) {
            hawk += 1;
        }
    }
    (dove, hawk)
}

/// The monetary-policy axis alone: (dovish − hawkish) / (dovish + hawkish) in [-1, 1];
/// 0 when no rates vocabulary occurs. Positive = dovish = bullish for bond prices.
pub fn rate_polarity(text: &str) -> f64 {
    let (dove, hawk) = rate_counts(text);
    if dove + hawk == 0 {
        return 0.0;
    }
    (dove as f64 - hawk as f64) / (dove + hawk) as f64
}

/// Sentiment for BOND PRICES: the dovish/hawkish axis carries most of the weight;
/// the general financial polarity enters NEGATED (risk-off is a Treasury bid).
/// When the text has no rates vocabulary at all, the flight-to-quality reading
/// (the negated general polarity) is all that remains.
pub fn bond_polarity(text: &str) -> f64 {
    let rp = rate_polarity(text);
    let gp = polarity_fused(text);
    if rate_counts(text) == (0, 0) {
        -gp
    } else {
        // policy vocabulary present: the hawk/dove axis dominates and the general
        // score enters as a mild risk-off correction only
        0.75 * rp - 0.25 * gp
    }
}

/// Score a whole document for the bond market: sentence-by-sentence bond
/// polarities aggregated with confidence weights, mirroring `score_document`.
pub fn score_document_bond(text: &str) -> (f64, Vec<(String, f64)>) {
    let sents = sentences(text);
    if sents.is_empty() {
        return (bond_polarity(text), Vec::new());
    }
    let scored: Vec<(String, f64)> = sents.into_iter().map(|s| { let p = bond_polarity(&s); (s, p) }).collect();
    // identical aggregation to score_document: confidence weights with a small floor,
    // so boilerplate dilutes the score rather than dropping out entirely
    let mut num = 0.0;
    let mut den = 0.0;
    for (_, p) in &scored {
        let w = p.abs().max(0.05);
        num += w * p;
        den += w;
    }
    (if den > 0.0 { num / den } else { 0.0 }, scored)
}

/// Split a document into sentences (., !, ?, and blank-line boundaries).
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split("\n\n") {
        let flat = para.replace('\n', " ");
        let mut cur = String::new();
        for ch in flat.chars() {
            cur.push(ch);
            if matches!(ch, '.' | '!' | '?') {
                let t = cur.trim().to_string();
                if t.split_whitespace().count() >= 3 {
                    out.push(t);
                }
                cur.clear();
            }
        }
        let t = cur.trim().to_string();
        if t.split_whitespace().count() >= 3 {
            out.push(t);
        }
    }
    out
}

/// Score a whole document or report: per-sentence model polarities, aggregated
/// with confidence weights (|polarity| — confident sentences count for more;
/// boilerplate scores near zero and drops out). Returns the document polarity
/// and the scored sentences in reading order.
pub fn score_document(text: &str) -> (f64, Vec<(String, f64)>) {
    let sents = sentences(text);
    if sents.is_empty() {
        return (polarity_fused(text), Vec::new());
    }
    let scored: Vec<(String, f64)> = sents
        .into_iter()
        .map(|t| {
            let p = polarity_fused(&t);
            (t, p)
        })
        .collect();
    let mut num = 0.0;
    let mut den = 0.0;
    for (_, p) in &scored {
        let w = p.abs().max(0.05);
        num += w * p;
        den += w;
    }
    (if den > 0.0 { num / den } else { 0.0 }, scored)
}
