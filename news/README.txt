The DOCUMENT ADVICE STREAM and the NEWSWIRE's home directory.

Documents: drop reports, articles, or notes here as .txt or .md files;
`latte trade` (and the GUI advisor) scores each one whole — sentence by
sentence, with the trained classifier — weights it by recency (5-day
half-life; name files YYYY-MM-DD-anything.txt to date them, else mtime is
used), and blends the aggregate into the news leg of the combined signal
(press 60% · documents 25% · social 15%, renormalized over what exists).

`latte fetch --news <url>` curls a document straight into this directory.

The NEWSWIRE fetches fresh press RSS and social streams automatically
(30-minute TTL; `latte news` shows the pulse, `latte news fetch` pulls now,
docs/newswire.md has the whole story). A `sources.tsv` file HERE overrides
the built-in source registry — one source per line:

  name<TAB>kind<TAB>format<TAB>trust<TAB>markets<TAB>url

  kind:    press | social
  format:  rss | reddit | hn | bsky
  trust:   0..1 (majors ~0.9, aggregators ~0.7, social ~0.5)
  markets: comma list of symbols, or *
