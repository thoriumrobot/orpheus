The DOCUMENT ADVICE STREAM. Drop reports, articles, or notes here as .txt or
.md files; `latte trade` (and the GUI advisor) scores each one whole — sentence
by sentence, with the trained classifier — weights it by recency (5-day
half-life; name files YYYY-MM-DD-anything.txt to date them, else mtime is
used), and blends the aggregate into the news leg of the combined signal
(documents 40%, headlines 60%).

`latte fetch --news <url>` curls a document straight into this directory.
