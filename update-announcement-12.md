# Orpheus update — the newswire: fresh news, social pulse, and scoring past sentiment

## News is now FRESH, automatically

Until now the advisors' news leg was a corpus frozen on the day it shipped, plus
whatever you pasted by hand. The **NEWSWIRE** (`latte news`) fixes that at the
mechanism level: a curated registry of press RSS feeds (Cointelegraph, Decrypt,
Bitcoin.com News, Bitcoin Magazine, CryptoSlate, NewsBTC) and open social streams
(Reddit's public listings, Hacker News, Bluesky search) — all plain-`curl`
fetchable, no API keys — lands in one deduplicated, age-pruned store. Freshness
is pull-based with a 30-minute TTL: `latte trade`, `latte news`, and
`GET /api/news` refresh the store themselves when it is stale, the GUI server
runs a background refresher for open dashboards, and every failure is soft (the
embedded corpus remains the floor, so nothing ever goes silent).
`news/sources.tsv` swaps in your own feeds; `ORPHEUS_NEWS_AUTO=0` turns the
automation off.

## Appropriate weights, spelled out

Every item's contribution is polarity × weight, where weight = source **trust**
(majors ~0.9, social ~0.5) × **engagement** (social posts scale by log-upvotes,
capped at 2× — the crowd is evidence, but it cannot swamp the press) ×
**relevance** (direct mention 1.0, macro context 0.6, else dropped) ×
**event impact** × **recency decay** (press half-life 3 days, social HALF A DAY —
buzz goes stale an order of magnitude faster than reporting). The advisor's news
leg composes press 60% · documents 25% · social 15%; the aggregate ratio is
computed on Loom by `lib/sentiment.lat`'s new `wagg` — the language stays in
the loop.

## Past plain sentiment: events and SESTM

Research has moved past one polarity number, and so has the scorer:

- **Event conditioning** (`src/events.rs`): a white-box taxonomy — etf-flow,
  regulation, hack, macro-rates, adoption, tech, market-structure, geopolitics,
  retail-buzz — tags every item, scales its weight (a hack counts 1.4×, moon-boys
  posts 0.6×), and sets its decay clock (regulation persists a week; a
  liquidation cascade is stale tomorrow). Tags print everywhere the item does.
- **SESTM** (Ke–Kelly–Xiu, *Predicting Returns with Text Data*): `latte news
  train` learns the lexicon from the market's OWN price responses — screen terms
  by up-day frequency, estimate topic weights, score by penalized likelihood.
  Honest framing throughout: it trains only with ≥ 60 dated wire items, reports
  say which engine scored what, and the trained classifier + Loughran-McDonald
  fusion remains the floor, so quality never drops for lack of data.

## Everywhere at once

The crypto advisor's press leg is the live wire (with a SOCIAL PULSE block);
the bond advisor reads the same wire through the hawk/dove axis (risk-on crypto
headlines invert — flight-to-quality — and macro-rates stories carry their
weight); `/api/news` serves the scored wire as JSON with labels and weights;
the `/trade` GUI page renders the wire tables; `latte news pulse` is the
standalone view. `--news FILE` still outranks everything, and the document
advice stream is unchanged.

## Causal routing, not keyword matching

Feeding "news containing *bond*" to the bond desk misses almost everything that
moves it. Every event class now carries per-market TRANSMISSION coefficients —
the causal channels: a Fed decision reaches bonds at 1.0 and crypto at 0.7
without naming either asset; a Treasury auction or a deficit story is a bond
story with only a debasement echo in crypto (0.35); a bank failure bids
Treasuries (0.85) while crypto trades both sides of it (0.55); an exchange hack
never reaches the duration desk at all. Relevance to a market is the strongest
of: the asset named, its sector named, a sibling coin named (diluted, 0.4), or
the event transmission — with a 0.2 floor below which items drop. The bond
advisor and `latte news pulse --market bonds` route through the bonds universe
and score on the hawk/dove axis: the auction tails read bearish, the bank
failure reads as the flight-to-quality bid, and the moon posts never appear.
Substring lies are word-bounded away ("war" must not fire on *warning*, "defi"
not on *deficit* — both found by tests).

## The VALUES wire: models predict on fresh numbers

The same TTL discipline now keeps the models' NUMBERS fresh (six hours; daily
series; `ORPHEUS_DATA_AUTO=0` disables). Crypto consumers auto-refresh the
cached Coin Metrics series. And the bond model's data finally goes live, exactly
as finbond.lat's header promised: FRED's keyless CSVs for DGS2/DGS5/DGS10
(month-ended) and M2SL (as YoY growth), aligned from 2007-01, cached
(`latte fetch --bonds`), and passed to the model's new `_on` arms — so
`latte trade --market bonds` trains on and predicts from today's actual curve,
with a `data:` provenance line saying which series served. The embedded teaching
anchors remain the no-network fallback, byte-for-byte (the delegating arms
reproduce the documented 69.6% out-of-sample edge exactly).

Tests: the four format parsers (RSS/Atom, Reddit, HN, Bluesky) with entity and
escape decoding, RFC-822 and ISO-8601 feed dates, log-engagement capping,
sources.tsv parsing, relevance tiers, the full weight product with event decay,
Loom-vs-direct aggregation agreement, wire round-trip with dedup, the event
taxonomy (including the discount-class fix: pure retail-buzz genuinely weighs
0.6×), SESTM training/scoring/persistence on a synthetic aligned corpus, its
refusal below the honesty gate, the trained-vs-untrained polarity blend,
the causal transmission map (auction/Fed/hack/bank-failure routing per desk,
word-boundary regressions), FRED CSV parsing with missing-value rows, month-end
downsampling, M2 YoY arithmetic, the bond cache round-trip, and the finbond
`_on` delegation (the embedded bond-desk tests pass unchanged).
