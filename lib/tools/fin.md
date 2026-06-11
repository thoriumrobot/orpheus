# Fin.Tool — the financial-ML pipeline
A logistic model on real market data; equity NET of 5bp costs; honest baselines throughout.

iters [field: iters=80]
[Volatility task](run: fin $iters) [Direction task + risk stats](run: fin --direction $iters) [The advisor](run: trade account=10000)
