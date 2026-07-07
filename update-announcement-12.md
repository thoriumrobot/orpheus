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

Tests: the four format parsers (RSS/Atom, Reddit, HN, Bluesky) with entity and
escape decoding, RFC-822 and ISO-8601 feed dates, log-engagement capping,
sources.tsv parsing, relevance tiers, the full weight product with event decay,
Loom-vs-direct aggregation agreement, wire round-trip with dedup, the event
taxonomy (including the discount-class fix: pure retail-buzz genuinely weighs
0.6×), SESTM training/scoring/persistence on a synthetic aligned corpus, its
refusal below the honesty gate, and the trained-vs-untrained polarity blend.
