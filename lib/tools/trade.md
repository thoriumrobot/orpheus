# Trade.Tool — the trading advisor
TA votes × news sentiment × HAR-RV volatility-targeted Kelly sizing. Reports embed below.

account [field: account=10000]  kelly [field: kelly=1.0]  market [field: market=btc]
[Advise](run: trade account=$account kelly=$kelly market=$market) [Advise live](run: trade account=$account kelly=$kelly market=$market live=1) [Technical analysis](run: ta market=$market) [Chart 180d](run: chart market market=$market days=180)

## Other markets
Fetch once from the CLI (`latte fetch --market eth`, or `--all`); then set the market field above
(eth ltc xrp ada doge sol …). Each market trains its own volatility model, and the registry
records its edge honestly (~/.cache/orpheus/models/).

## Score text and documents
headline [field: headline=etf outflows eased as markets steadied]
[Score headline](run: System.Sentiment $headline) [Score the ML doc](run: Doc.Score visualization-and-ml)

Or type any command line and middle-click it:
trade account=25000 kelly=0.5
