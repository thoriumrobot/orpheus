# Chart.Tool — visualization
Layout computed in lib/plot.lat on Loom; charts embed where you run them.

days [field: days=120]  market [field: market=btc]
[Market chart](run: chart market market=$market days=$days) [Live](run: chart market market=$market days=$days live=1)

[Bar demo](run: chart bar 3 1 4 1 5 9 2 6) [Line demo](run: chart line 10 12 9 14 17 13 18) [Scatter demo](run: chart scatter 3 7 1 9 4 6 8 2) [Rings](run: gfx Tool.rings 6)

chart bar 2 7 1 8 2 8
