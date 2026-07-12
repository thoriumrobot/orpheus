# The Newswire — fresh news and social posts, fetched automatically, scored event-aware

Orpheus's trading tools read text: headlines, reports, and social chatter feed the
sentiment leg of the trade advisors, the GUI dashboard, and the `/api` endpoints. The
NEWSWIRE (`src/newswire.rs` + `src/events.rs`) is the mechanism that keeps that text
FRESH without any manual step, and the scoring engine that goes past plain sentiment.

```
latte news              the pulse: the wire scored for a market, evidence shown
latte news fetch        pull every source into the wire store now
latte news train        fit the SESTM return-supervised model on the accumulated wire
latte news sources      the source registry (and how to override it)
latte trade             …consumes the wire automatically (so do bonds, the GUI, the API)
```

## What arrives, and from where

The built-in registry covers two kinds of source, all fetchable with plain `curl`
(the same shell-out trust model as `latte fetch` and Anvil's `rustc`), no API keys:

- **Press** — crypto news RSS/Atom feeds: Cointelegraph, Decrypt, Bitcoin.com News,
  Bitcoin Magazine, CryptoSlate, NewsBTC. Trust 0.70–0.90.
- **Social** — Reddit's public JSON listings (r/Bitcoin, r/CryptoCurrency), Hacker News
  (the Algolia API), and Bluesky's public search. Trust 0.45–0.55, engagement-weighted.

`news/sources.tsv` replaces the registry when present — one line per source:

```
# name	kind	format	trust	markets	url
myfeed	press	rss	0.8	btc	https://example.com/feed
mysub	social	reddit	0.5	*	https://www.reddit.com/r/somewhere/new.json?limit=40
```

(kind: `press`|`social`; format: `rss`|`reddit`|`hn`|`bsky`; markets: comma list or `*`.)
Social endpoints are best-effort by nature — Reddit throttles unauthenticated readers —
and every failure is soft: the wire keeps whatever it has, and the advisors keep working
(the embedded corpus is the floor, so nothing ever goes silent).

## Automatic freshness

Items land in `<cache>/news/wire.tsv` (30 days, deduplicated by a SHA3 of the normalized
headline). Freshness is pull-based with a TTL: any consumer — `latte trade`,
`latte news`, `GET /api/news` — refreshes the store when it is older than 30 minutes.
`--live` forces a fetch; `ORPHEUS_NEWS_AUTO=0` turns auto-fetch off (the store then only
moves on an explicit `latte news fetch`). The GUI server additionally runs a background
refresher thread, so an open dashboard stays fed. `ORPHEUS_CACHE` relocates the store;
`ORPHEUS_NEWS` relocates `sources.tsv` along with the document advice stream.

## The weights (what "appropriate" means here)

Every item's contribution is `polarity x weight`, with

```
weight = trust x engagement x relevance x event-impact x 0.5^(age / half-life)
```

- **trust** — the source's curated credibility (majors ~0.9, aggregators ~0.7, social ~0.5).
- **engagement** — social items scale by `1 + ln(1+upvotes)/8`, capped at 2: a
  thousand-upvote thread is real evidence of attention, but virality cannot do more than
  double a post, so the crowd never swamps the press.
- **relevance** — CAUSAL, not lexical (see the next section): direct mention 1.0, the
  asset class 0.8, a sibling coin 0.4, else the event-transmission map; below 0.2 drops.
- **event impact and half-life** — see below.
- **recency** — press headlines decay with the advisor's long-standing 3-day half-life;
  social chatter with HALF A DAY (buzz goes stale an order of magnitude faster than
  reporting); documents in `news/` keep their 5-day half-life.

The aggregate ratio `sum(w·p)/sum(w)` is computed on Loom by `lib/sentiment.lat`'s
`wagg` (signed fixed-point) — the same language-in-the-loop discipline as the original
polarity arithmetic.

Inside the advisors the news leg is composed **press 60% · documents 25% · social 15%**
(renormalized over the legs that exist), and the combined signal stays TA 60% / news 40%.
The bond advisor reads the same wire through the hawk/dove bond scorer (risk-off inverts,
`macro-rates` stories matter most) at wire 45% / documents 25%, with a fresh `--news`
document at 60% when supplied.

## Causal routing: the transmission map

"Relevant to a market" does not mean "contains the market's name". A Fed decision
never says *bitcoin*; a Treasury auction never says *bond*; both move their markets
anyway, through channels macro-finance has documented for decades. Every event class
in the taxonomy therefore carries two TRANSMISSION coefficients — how strongly that
kind of story impinges on **crypto** and on the **rates/duration** market:

| event class      | crypto | bonds | the channel |
|------------------|-------:|------:|-------------|
| macro-rates      | 0.70   | 1.00  | policy rates: mechanical for duration, discount-rate/liquidity for risk assets |
| fiscal-supply    | 0.35   | 1.00  | auctions, deficits, issuance: a bond story with a debasement echo in crypto |
| banking-credit   | 0.55   | 0.85  | flight to quality bids Treasuries; crypto trades risk-off *and* "not a bank" |
| fx-liquidity     | 0.75   | 0.70  | dollar, QE/QT, money supply: the shared liquidity tide |
| equity-risk      | 0.65   | 0.60  | risk appetite moves both, in opposite directions |
| geopolitics      | 0.60   | 0.75  | safe-haven bid vs risk-off hit |
| etf-flow         | 0.90   | 0.10  | fund flows are the marginal crypto buyer |
| hack             | 0.85   | 0.05  | crypto-native; the duration desk drops it |
| regulation       | 0.50   | 0.10  | crypto-legal narrative |
| adoption         | 0.70   | 0.05  | crypto-native |
| market-structure | 0.70   | 0.25  | liquidations/leverage, mostly crypto plumbing |
| tech             | 0.60   | 0.00  | protocol stories |
| retail-buzz      | 0.50   | 0.00  | moon-boys do not move Treasuries |

An item's relevance to a market is the STRONGEST of: the asset named (1.0), its
sector named (0.8 — crypto-wide terms for a crypto market, or the fixed-income complex
"credit spreads", "high yield", "sovereign", etc. for the bond desk), a sibling coin
named (0.4 — diluted evidence, crypto only), or the transmission of any matched event
class. Anything under 0.2 never reaches the advisor. So `latte news pulse --market bonds`
shows the auction, the bank failure, the FOMC decision, and the risk-off tape — scored on
the hawk/dove bond axis (risk-off = Treasury bid) — while the ETF flows, the exchange
hack, and the moon posts stay on the crypto desk. Patterns are word-bounded where
substrings would lie ("war" must not fire on *warning*, "defi" not on *deficit*).

## The VALUES wire: fresh numbers for the models

Sentiment is half the input; the models also need fresh PRICES. The same TTL
discipline now covers the numbers (`ORPHEUS_DATA_AUTO=0` disables; six-hour TTL —
the series are daily):

- **Crypto** — any consumer of the cached Coin Metrics series (`latte trade`, `ta`,
  `chart`, the GUI) refreshes it automatically when it is older than the TTL; the
  embedded series remains the floor. `--live` still forces.
- **Bonds** — the model's data source finally goes live, exactly as finbond.lat's
  header always promised: FRED's keyless CSV endpoints for **DGS2 / DGS5 / DGS10**
  (daily constant-maturity Treasury yields, downsampled to month-end) and **M2SL**
  (money stock, turned into year-over-year growth), aligned monthly from 2007-01
  and cached (`latte fetch --bonds` forces; the TTL refreshes it otherwise). The
  host passes the four series to the model's `_on` arms (`badvice_on`,
  `bdrivers_on`, `bvol_on` — the embedded arms delegate to the same code), so
  `latte trade --market bonds` trains on and PREDICTS FROM today's actual curve.
  The report's `data:` line says which series served. No cache, no network? The
  embedded teaching anchors keep the model running, and the report says so.

## Past plain sentiment: events + SESTM

The research verdict is that one polarity number is too coarse, in two specific ways —
and both fixes are white-box enough to live inside a zero-dependency system:

1. **Event conditioning.** Different narrative types have different magnitudes and decay
   rates (the event-factor literature extends SESTM exactly this way). `src/events.rs`
   tags each item with a finance/crypto taxonomy — `etf-flow` (impact 1.30, half-life 2d),
   `regulation` (1.20, 7d), `hack` (1.40, 5d), `macro-rates` (1.10, 3d), `adoption`
   (1.15, 7d), `tech` (0.90, 7d), `market-structure` (1.20, 1d), `geopolitics` (1.00, 2d),
   `retail-buzz` (0.60, 12h) — and the matched class scales the item's weight and sets
   its decay clock. A hack outlives the news cycle; a moon-boys post barely survives
   lunch. Tags are shown everywhere the item is (`[etf-flow adoption]`), so the
   conditioning is inspectable.

2. **SESTM** — Sentiment Extraction via Screening and Topic Modeling (Ke, Kelly & Xiu,
   "Predicting Returns with Text Data", NBER w26186): learn the lexicon FROM the
   market's own price responses instead of from human labels. `latte news train`
   aligns the wire's dated items with the freshest price series, labels each by the
   next day's return sign, screens terms whose up-day frequency sits far from 1/2
   (kappa=3 occurrences, alpha=0.15), estimates positive/negative topic weights with
   Laplace smoothing, and scores a new text by penalized maximum likelihood over the
   article's tone parameter p — a one-dimensional, fully inspectable optimization.
   The model persists per market (`<cache>/news/sestm-<sym>.tsv`).

   HONEST FRAMING: SESTM only participates once it has ≥ 60 dated items that match the
   series — `latte news train` says exactly how many it had, and every pulse report says
   which engine scored it. When trained, an item's polarity is
   `0.5·SESTM + 0.5·(classifier+lexicon)`; when SESTM has no screened term in a text it
   abstains and the fused classifier score stands alone. The trained
   classifier + Loughran-McDonald engine (see `docs/visualization-and-ml.md`) remains
   the floor, so scoring quality never goes DOWN for lack of data.

## Where it flows

Everything that read news before now reads the wire, weights included:

- `latte trade [--market SYM]` — the press leg is the live wire when it has relevant
  items (an explicit `--news FILE` still outranks it; the embedded corpus is the fallback),
  and a SOCIAL PULSE block appears when the wire has social items.
- `latte trade --market bonds` — the wire routed through the BONDS causal universe and
  scored on the hawk/dove axis, the model trained on live FRED yields when cached.
- `GET /api/news[?market=SYM][&fresh=1]` — the scored wire as JSON (kind, labels, weight,
  press/social/combined aggregates), embedded-corpus fallback so it always answers.
- `POST /api/trade` (the `/trade` GUI page) — renders the wire's press table, the wire
  provenance line, and the social pulse table.
- `latte news` / `latte news pulse` — the standalone view with per-item evidence.

`latte fetch --news <url>` (a whole document into `news/`) and the document advice
stream are unchanged — documents age slowly and blend at 25%.
