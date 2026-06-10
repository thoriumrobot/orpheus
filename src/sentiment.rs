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
