//! newswire — FRESH news and social posts, fetched automatically, weighted appropriately.
//!
//! Before this module, the advisor's news leg was frozen: an embedded corpus gathered on one
//! day, plus whatever a user pasted by hand. The wire makes freshness automatic:
//!
//! - **Sources.** A curated registry of press RSS/Atom feeds (Cointelegraph, Decrypt,
//!   Bitcoin.com News, NewsBTC, CryptoSlate, Bitcoin Magazine) and SOCIAL streams (Reddit's
//!   public JSON listings, Hacker News via the Algolia API, Bluesky's public search) — all
//!   fetchable with plain `curl`, no API keys. `news/sources.tsv` overrides the registry
//!   (one line per source: name, kind, format, trust, markets, url), so any feed can join.
//! - **Transport.** HTTPS shells out to `curl`, the same trust model as `latte fetch` and
//!   Anvil shelling out to `rustc`; every failure is soft (a note, never an error) and the
//!   system falls back to whatever it already has — the embedded corpus in the worst case,
//!   so the advisors always answer.
//! - **The wire store.** Items land in `<cache>/news/wire.tsv`, deduplicated by a SHA3 of
//!   the normalized headline, pruned by age. One store feeds every consumer.
//! - **Automatic.** Consumers call `market_wire`, which refreshes the store when it is
//!   older than the TTL (30 min; `ORPHEUS_NEWS_AUTO=0` disables auto-fetch, `--live`
//!   forces one). The GUI server additionally runs a background refresher thread, so an
//!   open dashboard stays fresh without any CLI action.
//! - **Weights.** Every item carries the components of its aggregation weight:
//!   source TRUST (curated per source: majors ~0.9, aggregators ~0.7, social base ~0.5),
//!   ENGAGEMENT (social items scale by log of upvotes/points — a 2,000-upvote thread is
//!   evidence of attention in a way a 2-upvote one is not; capped so virality cannot
//!   swamp the press), RELEVANCE to the market asked about (direct mention 1.0, macro
//!   context 0.6), and the EVENT conditioning from src/events.rs (impact multiplier and
//!   per-event half-life driving the recency decay). Press headlines default to the
//!   3-day half-life the advisor has always used; social chatter defaults to half a DAY —
//!   buzz decays an order of magnitude faster than reporting.
//!
//! The aggregate each consumer receives is sum(w_i * p_i) / sum(w_i), with the ratio
//! computed on Loom (lib/sentiment.lat `wagg`) — the language stays in the loop, exactly
//! as the original polarity arithmetic did.

use crate::events;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Press,
    Social,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Format {
    Rss,        // RSS 2.0 or Atom
    RedditJson, // reddit.com/r/<sub>/new.json listings
    HnJson,     // hn.algolia.com search_by_date
    BskyJson,   // public.api.bsky.app searchPosts
}

pub struct Source {
    pub name: String,
    pub kind: Kind,
    pub format: Format,
    pub trust: f64,
    pub markets: String, // comma list of symbols this source informs; "*" = all
    pub url: String,
}

/// One wire item. `engagement` is the log-scaled attention factor (1.0 for press);
/// `epoch` is Unix seconds (feed timestamps when given, fetch time otherwise).
#[derive(Clone)]
pub struct Item {
    pub epoch: i64,
    pub date: String,
    pub kind: Kind,
    pub trust: f64,
    pub engagement: f64,
    pub source: String,
    pub headline: String,
}

/// The built-in source registry: crypto press + the open social streams. All of these are
/// keyless; the social entries are best-effort by nature (Reddit throttles unauthenticated
/// readers to ~10 req/min and may refuse — the wire polls each source once per refresh,
/// far under that, and shrugs off refusals). Users extend or replace this via
/// `news/sources.tsv`.
pub fn default_sources() -> Vec<Source> {
    let s = |name: &str, kind: Kind, format: Format, trust: f64, markets: &str, url: &str| Source {
        name: name.into(), kind, format, trust, markets: markets.into(), url: url.into(),
    };
    vec![
        s("cointelegraph", Kind::Press, Format::Rss, 0.90, "*", "https://cointelegraph.com/rss"),
        s("decrypt", Kind::Press, Format::Rss, 0.85, "*", "https://decrypt.co/feed"),
        s("bitcoin.com", Kind::Press, Format::Rss, 0.80, "*", "https://news.bitcoin.com/feed"),
        s("bitcoinmagazine", Kind::Press, Format::Rss, 0.80, "btc", "https://bitcoinmagazine.com/feed"),
        s("cryptoslate", Kind::Press, Format::Rss, 0.75, "*", "https://cryptoslate.com/feed"),
        s("newsbtc", Kind::Press, Format::Rss, 0.70, "*", "https://www.newsbtc.com/feed"),
        // the macro/rates channel: the Fed's own press feed and a broad economy desk —
        // this is where the bond advisor's causal stories (and crypto's liquidity
        // stories) come from, since crypto outlets rarely cover an auction
        s("federalreserve", Kind::Press, Format::Rss, 0.95, "*",
          "https://www.federalreserve.gov/feeds/press_all.xml"),
        s("cnbc-economy", Kind::Press, Format::Rss, 0.80, "*",
          "https://www.cnbc.com/id/20910258/device/rss/rss.html"),
        s("r/Bitcoin", Kind::Social, Format::RedditJson, 0.50, "btc",
          "https://www.reddit.com/r/Bitcoin/new.json?limit=40"),
        s("r/CryptoCurrency", Kind::Social, Format::RedditJson, 0.50, "*",
          "https://www.reddit.com/r/CryptoCurrency/new.json?limit=40"),
        s("hackernews", Kind::Social, Format::HnJson, 0.55, "btc",
          "https://hn.algolia.com/api/v1/search_by_date?query=bitcoin&tags=story&hitsPerPage=30"),
        s("bluesky", Kind::Social, Format::BskyJson, 0.45, "btc",
          "https://public.api.bsky.app/xrpc/app.bsky.feed.searchPosts?q=bitcoin&limit=30"),
    ]
}

/// Parse a `news/sources.tsv` override. Lines: name<TAB>kind<TAB>format<TAB>trust<TAB>markets<TAB>url
/// (kind: press|social; format: rss|reddit|hn|bsky). `#` comments and blanks skipped.
pub fn parse_sources(text: &str) -> Vec<Source> {
    let mut out = Vec::new();
    for l in text.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 6 {
            continue;
        }
        let kind = match f[1] {
            "press" => Kind::Press,
            "social" => Kind::Social,
            _ => continue,
        };
        let format = match f[2] {
            "rss" => Format::Rss,
            "reddit" => Format::RedditJson,
            "hn" => Format::HnJson,
            "bsky" => Format::BskyJson,
            _ => continue,
        };
        let trust = match f[3].parse::<f64>() {
            Ok(t) if (0.0..=1.0).contains(&t) => t,
            _ => continue,
        };
        out.push(Source {
            name: f[0].into(), kind, format, trust, markets: f[4].into(), url: f[5].into(),
        });
    }
    out
}

/// The active sources: `news/sources.tsv` when present (same directory discipline as the
/// document advice stream, so `ORPHEUS_NEWS` relocates both), else the built-in registry.
pub fn sources() -> Vec<Source> {
    let path = crate::numerics::news_dir().join("sources.tsv");
    match std::fs::read_to_string(&path) {
        Ok(t) => {
            let v = parse_sources(&t);
            if v.is_empty() { default_sources() } else { v }
        }
        Err(_) => default_sources(),
    }
}

// ---------------------------------------------------------------------------
// Parsers — zero-dependency, best-effort extraction from the four wire formats.
// ---------------------------------------------------------------------------

/// Decode the HTML/XML entities that occur in feed titles.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'&' {
            if let Some(end) = s[i..].find(';').map(|e| i + e) {
                let ent = &s[i + 1..end];
                let rep = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    _ if ent.starts_with('#') => {
                        let n = if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                            u32::from_str_radix(hex, 16).ok()
                        } else {
                            ent[1..].parse::<u32>().ok()
                        };
                        n.and_then(char::from_u32)
                    }
                    _ => None,
                };
                if let Some(c) = rep {
                    if end - i <= 10 {
                        out.push(c);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The text of the first `<tag>…</tag>` inside `block`, CDATA unwrapped, entities decoded,
/// whitespace collapsed.
fn xml_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let s = block.find(&open)?;
    let after = block[s..].find('>')? + s + 1;
    let e = block[after..].find(&close)? + after;
    let mut inner = block[after..e].trim().to_string();
    if let Some(c) = inner.strip_prefix("<![CDATA[") {
        inner = c.strip_suffix("]]>").unwrap_or(c).trim().to_string();
    }
    let flat: String = decode_entities(&inner).split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() { None } else { Some(flat) }
}

/// RFC 822 feed dates ("Mon, 06 Jul 2026 14:00:00 +0000" / "GMT") and ISO 8601
/// ("2026-07-06T14:00:00Z" / "+00:00" / fractional seconds) to Unix seconds.
pub fn parse_feed_date(s: &str) -> Option<i64> {
    let s = s.trim();
    // ISO 8601: YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]
    if s.len() >= 19 && s.as_bytes()[4] == b'-' && (s.as_bytes()[10] == b'T' || s.as_bytes()[10] == b' ') {
        let (y, mo, d) = (s.get(0..4)?.parse::<i64>().ok()?, s.get(5..7)?.parse::<i64>().ok()?, s.get(8..10)?.parse::<i64>().ok()?);
        let (h, mi, sec) = (s.get(11..13)?.parse::<i64>().ok()?, s.get(14..16)?.parse::<i64>().ok()?, s.get(17..19)?.parse::<i64>().ok()?);
        let mut off = 0i64;
        let rest = s.get(19..).unwrap_or("");
        let tz = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
        if let Some(sign) = tz.chars().next() {
            if sign == '+' || sign == '-' {
                let hh: i64 = tz.get(1..3).and_then(|x| x.parse().ok()).unwrap_or(0);
                let mm: i64 = tz.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(0);
                off = (hh * 3600 + mm * 60) * if sign == '+' { 1 } else { -1 };
            }
        }
        let days = crate::dates::date_ordinal(&format!("{:04}-{:02}-{:02}", y, mo, d))?;
        return Some(days * 86_400 + h * 3600 + mi * 60 + sec - off);
    }
    // RFC 822: [Www, ]DD Mon YYYY HH:MM:SS ZZZ
    let parts: Vec<&str> = s.split_whitespace().collect();
    let i = if parts.first().map(|p| p.ends_with(',')).unwrap_or(false) { 1 } else { 0 };
    if parts.len() < i + 4 {
        return None;
    }
    let d: i64 = parts[i].parse().ok()?;
    const MONTHS: &[&str] = &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mo = MONTHS.iter().position(|m| parts[i + 1].starts_with(m))? as i64 + 1;
    let y: i64 = parts[i + 2].parse().ok()?;
    let hms: Vec<i64> = parts[i + 3].split(':').filter_map(|x| x.parse().ok()).collect();
    let (h, mi, sec) = (*hms.first()? , *hms.get(1)?, *hms.get(2).unwrap_or(&0));
    let mut off = 0i64;
    if let Some(tz) = parts.get(i + 4) {
        let b = tz.as_bytes();
        if (b[0] == b'+' || b[0] == b'-') && tz.len() == 5 {
            let hh: i64 = tz[1..3].parse().unwrap_or(0);
            let mm: i64 = tz[3..5].parse().unwrap_or(0);
            off = (hh * 3600 + mm * 60) * if b[0] == b'+' { 1 } else { -1 };
        }
    }
    let days = crate::dates::date_ordinal(&format!("{:04}-{:02}-{:02}", y, mo, d))?;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec - off)
}

/// RSS 2.0 `<item>` and Atom `<entry>` titles with their timestamps.
pub fn parse_rss(xml: &str, now: i64) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    for (open, close) in [("<item", "</item>"), ("<entry", "</entry>")] {
        let mut rest = xml;
        while let Some(s) = rest.find(open) {
            let after = &rest[s..];
            let e = match after.find(close) {
                Some(e) => e,
                None => break,
            };
            let block = &after[..e];
            if let Some(title) = xml_tag(block, "title") {
                let ts = ["pubDate", "published", "updated", "dc:date"]
                    .iter()
                    .find_map(|t| xml_tag(block, t))
                    .and_then(|d| parse_feed_date(&d))
                    .unwrap_or(now);
                out.push((ts, title));
            }
            rest = &after[e + close.len()..];
        }
        if !out.is_empty() {
            break; // an RSS feed's <item>s found; don't re-scan as Atom
        }
    }
    out
}

/// The JSON string value that follows `"key":` at or after `from`; handles \" \\ \n \t \/
/// and \uXXXX escapes. Returns (value, index just past the closing quote).
fn json_str_after(hay: &str, key: &str, from: usize) -> Option<(String, usize)> {
    let pat = format!("\"{}\"", key);
    let k = hay[from..].find(&pat)? + from + pat.len();
    let colon = hay[k..].find(':')? + k + 1;
    let rest = hay[colon..].trim_start();
    let off = colon + (hay[colon..].len() - rest.len());
    if !rest.starts_with('"') {
        return None;
    }
    let b = rest.as_bytes();
    let mut i = 1;
    let mut s = String::new();
    while i < b.len() {
        match b[i] {
            b'"' => return Some((s, off + i + 1)),
            b'\\' if i + 1 < b.len() => {
                i += 1;
                match b[i] {
                    b'n' => s.push(' '),
                    b't' => s.push(' '),
                    b'r' => {}
                    b'u' if i + 4 < b.len() => {
                        if let Ok(n) = u32::from_str_radix(&rest[i + 1..i + 5], 16) {
                            if let Some(c) = char::from_u32(n) {
                                s.push(c);
                            }
                        }
                        i += 4;
                    }
                    // an escaped ASCII byte (\" \\ \/ etc). A backslash before a
                    // multibyte character is malformed JSON; treat the backslash as a
                    // no-op and let the following bytes be decoded normally below.
                    c if c < 0x80 => s.push(c as char),
                    _ => { i -= 1; } // not a real escape — reprocess this byte as text
                }
                i += 1;
            }
            c if c < 0x80 => {
                s.push(c as char);
                i += 1;
            }
            _ => {
                // a multibyte UTF-8 sequence starting at i: copy the whole char, advancing
                // by its byte length so `i` never lands inside a character (which would
                // panic when slicing). `rest[i..]` is safe here because `i` is always on a
                // char boundary at the top of the loop.
                match rest[i..].chars().next() {
                    Some(ch) => {
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                    None => i += 1, // unreachable given i < len, but never slice-panic
                }
            }
        }
    }
    None
}

/// The JSON number following `"key":` at or after `from`. Returns (value, index past it).
fn json_num_after(hay: &str, key: &str, from: usize) -> Option<(f64, usize)> {
    let pat = format!("\"{}\"", key);
    let k = hay[from..].find(&pat)? + from + pat.len();
    let colon = hay[k..].find(':')? + k + 1;
    let rest = hay[colon..].trim_start();
    let off = colon + (hay[colon..].len() - rest.len());
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E'))
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().ok().map(|v| (v, off + end))
}

/// Reddit listing JSON: each post's `title`, `score`, `created_utc` (they appear in that
/// order within a post's `data` object). Returns (epoch, upvotes, title) rows.
pub fn parse_reddit(json: &str) -> Vec<(i64, f64, String)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some((title, next)) = json_str_after(json, "title", at) {
        let score = json_num_after(json, "score", next);
        let created = score.as_ref().and_then(|(_, n)| json_num_after(json, "created_utc", *n));
        match (score, created) {
            (Some((sc, _)), Some((ts, n2))) => {
                out.push((ts as i64, sc.max(0.0), title));
                at = n2;
            }
            _ => break,
        }
    }
    out
}

/// Hacker News (Algolia) hits: `created_at` (ISO), `title`, `points` per hit, in that order.
pub fn parse_hn(json: &str) -> Vec<(i64, f64, String)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some((created, n0)) = json_str_after(json, "created_at", at) {
        let title = json_str_after(json, "title", n0);
        let points = title.as_ref().and_then(|(_, n)| json_num_after(json, "points", *n));
        match (title, points, parse_feed_date(&created)) {
            (Some((t, _)), Some((p, n2)), Some(ts)) => {
                out.push((ts, p.max(0.0), t));
                at = n2;
            }
            _ => break,
        }
    }
    out
}

/// Bluesky searchPosts: each post's record `text` + `createdAt`, then `likeCount`.
pub fn parse_bsky(json: &str) -> Vec<(i64, f64, String)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some((text, n0)) = json_str_after(json, "text", at) {
        let created = json_str_after(json, "createdAt", n0);
        let likes = created.as_ref().and_then(|(_, n)| json_num_after(json, "likeCount", *n));
        match (created.as_ref().and_then(|(c, _)| parse_feed_date(c)), likes) {
            (Some(ts), Some((lk, n2))) => {
                let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if flat.len() >= 20 {
                    out.push((ts, lk.max(0.0), flat.chars().take(280).collect()));
                }
                at = n2;
            }
            _ => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The wire store: fetch, dedup, prune, load.
// ---------------------------------------------------------------------------

pub fn news_cache_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("ORPHEUS_CACHE") {
        return std::path::PathBuf::from(d).join("news");
    }
    let home = std::env::var("HOME").or_else(|_| std::env::var("LOCALAPPDATA")).unwrap_or_else(|_| ".".into());
    if cfg!(windows) {
        std::path::PathBuf::from(home).join("orpheus").join("news")
    } else {
        std::path::PathBuf::from(home).join(".cache").join("orpheus").join("news")
    }
}

fn wire_path() -> std::path::PathBuf {
    news_cache_dir().join("wire.tsv")
}
fn stamp_path() -> std::path::PathBuf {
    news_cache_dir().join("last-fetch")
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The dedup key: SHA3 of the lowercased alphanumerics of the headline (so retitled
/// whitespace/punctuation variants of one story collapse).
fn item_key(headline: &str) -> String {
    let norm: String = headline.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    crate::sha3::hex(&crate::sha3::sha3_256(norm.as_bytes())[..8])
}

/// Social engagement -> weight factor: 1 + ln(1 + n)/8, capped at 2. A thousand upvotes
/// roughly doubles a post's weight; virality cannot do more than that.
pub fn engagement_factor(score: f64) -> f64 {
    (1.0 + (1.0 + score.max(0.0)).ln() / 8.0).min(2.0)
}

const WIRE_MAX_AGE_DAYS: i64 = 30;
const WIRE_MAX_ITEMS: usize = 2000;

fn sanitize(headline: &str) -> String {
    headline.replace(['\t', '\n', '\r'], " ").trim().to_string()
}

/// Load the wire store, newest first.
pub fn load_wire() -> Vec<Item> {
    let text = match std::fs::read_to_string(wire_path()) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 7 {
            continue;
        }
        let (epoch, trust, engagement) = match (f[0].parse(), f[3].parse(), f[4].parse()) {
            (Ok(e), Ok(t), Ok(g)) => (e, t, g),
            _ => continue,
        };
        let kind = if f[2] == "social" { Kind::Social } else { Kind::Press };
        out.push(Item {
            epoch,
            date: f[1].to_string(),
            kind,
            trust,
            engagement,
            source: f[5].to_string(),
            headline: f[6].to_string(),
        });
    }
    out.sort_by(|a, b| b.epoch.cmp(&a.epoch));
    out
}

fn save_wire(items: &[Item]) {
    let dir = news_cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut s = String::new();
    for it in items.iter().take(WIRE_MAX_ITEMS) {
        s.push_str(&format!(
            "{}\t{}\t{}\t{:.3}\t{:.3}\t{}\t{}\n",
            it.epoch,
            it.date,
            if it.kind == Kind::Social { "social" } else { "press" },
            it.trust,
            it.engagement,
            it.source.replace('\t', " "),
            sanitize(&it.headline)
        ));
    }
    // Atomic replace: write a temp file then rename over the store, so a concurrent
    // reader (market_wire, /api/news, or another fetcher) never sees a half-written
    // wire.tsv — the same temp-then-rename discipline as store.rs and dbservice.rs.
    let final_path = wire_path();
    let tmp = dir.join(format!("wire.tsv.tmp.{}", std::process::id()));
    if std::fs::write(&tmp, &s).is_ok() {
        if std::fs::rename(&tmp, &final_path).is_err() {
            let _ = std::fs::write(&final_path, &s); // fallback if rename is unsupported
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// One `curl` fetch. A descriptive User-Agent is required by Reddit and polite everywhere.
fn curl(url: &str) -> Result<String, String> {
    let out = std::process::Command::new("curl")
        .args(["-sSL", "--max-time", "20", "-A", "orpheus-newswire/1.0 (research; zero-dependency)", url])
        .output()
        .map_err(|e| format!("curl not available ({})", e))?;
    if !out.status.success() {
        return Err(format!("curl exited with {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Fetch every source, merge new items into the wire (deduplicated, age-pruned), stamp the
/// time. Returns per-source notes for the report; the store is updated even when some
/// sources fail — freshness is best-effort by design.
pub fn fetch_all() -> Vec<String> {
    // One fetch at a time per process: the background refresher thread and an on-demand
    // market_wire() call must not interleave their load→merge→save cycles, or the second
    // save would clobber the first's new items (last-write-wins on the whole file). This
    // mutex makes the read-modify-write atomic within the process; the temp-then-rename in
    // save_wire covers cross-process readers.
    use std::sync::{Mutex, OnceLock};
    static FETCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = FETCH_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner());
    let now = now_epoch();
    let mut notes = Vec::new();
    let mut wire = load_wire();
    let mut seen: std::collections::HashSet<String> =
        wire.iter().map(|i| item_key(&i.headline)).collect();
    for src in sources() {
        let body = match curl(&src.url) {
            Ok(b) => b,
            Err(e) => {
                notes.push(format!("{:<18} {}", src.name, e));
                continue;
            }
        };
        let rows: Vec<(i64, f64, String)> = match src.format {
            Format::Rss => parse_rss(&body, now).into_iter().map(|(t, h)| (t, 0.0, h)).collect(),
            Format::RedditJson => parse_reddit(&body),
            Format::HnJson => parse_hn(&body),
            Format::BskyJson => parse_bsky(&body),
        };
        if rows.is_empty() {
            notes.push(format!("{:<18} fetched but nothing parsed", src.name));
            continue;
        }
        let mut added = 0usize;
        for (epoch, score, headline) in rows {
            let headline = sanitize(&headline);
            if headline.len() < 12 {
                continue;
            }
            if now - epoch > WIRE_MAX_AGE_DAYS * 86_400 || epoch > now + 3600 {
                continue;
            }
            let key = item_key(&headline);
            if !seen.insert(key) {
                continue;
            }
            let engagement = match src.kind {
                Kind::Press => 1.0,
                Kind::Social => engagement_factor(score),
            };
            wire.push(Item {
                epoch,
                date: crate::dates::ordinal_date(epoch / 86_400),
                kind: src.kind,
                trust: src.trust,
                engagement,
                source: src.name.clone(),
                headline,
            });
            added += 1;
        }
        notes.push(format!("{:<18} +{} new item(s)", src.name, added));
    }
    wire.retain(|i| now - i.epoch <= WIRE_MAX_AGE_DAYS * 86_400);
    wire.sort_by(|a, b| b.epoch.cmp(&a.epoch));
    save_wire(&wire);
    let _ = std::fs::write(stamp_path(), now.to_string());
    notes
}

/// Age of the last fetch in seconds; None when the wire has never fetched.
pub fn wire_age() -> Option<i64> {
    let t: i64 = std::fs::read_to_string(stamp_path()).ok()?.trim().parse().ok()?;
    Some((now_epoch() - t).max(0))
}

/// The automatic-freshness gate: refresh when the wire is stale (or `force`), unless
/// disabled (`ORPHEUS_NEWS_AUTO=0`) or running under the test harness (a test suite must
/// never reach the network). Returns a provenance note.
pub fn ensure_fresh(ttl_secs: i64, force: bool) -> String {
    if cfg!(test) {
        return "wire: test build, no fetch".into();
    }
    if std::env::var("ORPHEUS_NEWS_AUTO").map(|v| v == "0").unwrap_or(false) && !force {
        return match wire_age() {
            Some(a) => format!("wire: auto-fetch off, store is {} min old", a / 60),
            None => "wire: auto-fetch off, store empty".into(),
        };
    }
    let stale = match wire_age() {
        Some(a) => a >= ttl_secs,
        None => true,
    };
    if force || stale {
        let notes = fetch_all();
        let ok = notes.iter().filter(|n| n.contains("+")).count();
        format!("wire: refreshed ({}/{} sources answered)", ok, notes.len())
    } else {
        format!("wire: fresh ({} min old)", wire_age().unwrap_or(0) / 60)
    }
}

// ---------------------------------------------------------------------------
// Relevance, weighting, and the scored market view every consumer uses.
// ---------------------------------------------------------------------------

/// Keywords that make an item DIRECTLY about a market (relevance 1.0).
fn market_terms(market: &str) -> Vec<&'static str> {
    match market {
        "btc" => vec!["bitcoin", "btc"],
        "eth" => vec!["ethereum", " eth "],
        "ltc" => vec!["litecoin"],
        "xrp" => vec!["xrp", "ripple"],
        "ada" => vec!["cardano", " ada "],
        "doge" => vec!["dogecoin", "doge"],
        "sol" => vec!["solana", " sol "],
        "bonds" => vec!["treasur", "bond ", "bonds", "t-note", "t-bill", "10-year", "2-year",
                        "fixed income", "duration", "yield curve", "gilt", "bund"],
        _ => vec![],
    }
}

/// Which transmission column a market reads.
fn market_class(market: &str) -> crate::events::MarketClass {
    match market {
        "bonds" => crate::events::MarketClass::Bonds,
        _ => crate::events::MarketClass::Crypto,
    }
}

/// Sector terms: not this asset by name, but its asset class — crypto-wide news impinges
/// on every crypto market (BTC is ~half the asset class and correlations are high).
const CRYPTO_SECTOR: &[&str] = &["crypto", "blockchain", "digital asset", "altcoin", "defi ",
                                 "stablecoin", "token", "web3", "mining", "miner"];

/// Fixed-income sector terms: not a specific instrument by name, but the rates/credit
/// complex a bond desk trades — a story about "credit spreads" or "fixed-income flows"
/// impinges on the duration book even without saying "treasury".
const BOND_SECTOR: &[&str] = &["fixed income", "fixed-income", "credit spread", "credit market",
                               "sovereign", "coupon", "duration", "investment grade",
                               "high yield", "junk bond", "corporate debt", "bond market"];

/// Below this, an item does not reach a market's advisor at all.
const RELEVANCE_FLOOR: f64 = 0.2;

/// The relevance of a headline to a market — CAUSAL, not lexical. The question is not
/// "does the text say bitcoin/bond" but "does this kind of story move that market", and
/// it is answered in tiers, taking the strongest:
///   1.0  the asset is named (market_terms)
///   0.8  its sector is (crypto-wide news for a crypto market)
///   0.4  a sibling asset is named (cross-crypto spillover: correlations are high, but a
///        story about one coin is diluted evidence about another)
///   else the EVENT TRANSMISSION map (events::causal): a Fed decision reaches bonds at
///        1.0 and crypto at 0.7 without naming either; a Treasury auction reaches bonds;
///        an exchange hack reaches crypto and not the duration desk. Zero when no
///        recognized narrative is present.
/// Anything under RELEVANCE_FLOOR drops.
fn relevance(headline: &str, market: &str) -> f64 {
    let low = format!(" {} ", headline.to_lowercase());
    if market_terms(market).iter().any(|t| low.contains(t)) {
        return 1.0;
    }
    let class = market_class(market);
    let mut r = crate::events::causal(headline, class);
    if class == crate::events::MarketClass::Crypto {
        if CRYPTO_SECTOR.iter().any(|t| low.contains(t)) {
            r = r.max(0.8);
        } else {
            // sibling-coin spillover: another crypto named, this one not
            const SIBLINGS: &[&str] = &["btc", "eth", "ltc", "xrp", "ada", "doge", "sol"];
            let named_other = SIBLINGS.iter().any(|s| {
                *s != market && market_terms(s).iter().any(|t| low.contains(t))
            });
            if named_other {
                r = r.max(0.4);
            }
        }
    } else if class == crate::events::MarketClass::Bonds
        && BOND_SECTOR.iter().any(|t| low.contains(t))
    {
        // the fixed-income complex named without a specific instrument
        r = r.max(0.8);
    }
    if r < RELEVANCE_FLOOR {
        0.0
    } else {
        r
    }
}

/// One scored wire item as a consumer sees it.
pub struct ScoredItem {
    pub item: Item,
    pub polarity: f64,
    pub weight: f64,
    pub labels: Vec<&'static str>,
}

/// Score a set of items for a market: per-item polarity (events::item_polarity — the fused
/// classifier + SESTM blend, or the bond scorer), and per-item weight
/// trust x engagement x relevance x event-impact x 0.5^(age / event-half-life).
/// Items with zero relevance drop. The aggregate ratio runs on Loom (`wagg`).
pub fn score_items(items: &[Item], market: &str, bond: bool, now: i64) -> (Vec<ScoredItem>, f64) {
    let model = if bond { None } else { events::Sestm::load(&events::model_path(market)) };
    let mut rows = Vec::new();
    for it in items {
        let rel = relevance(&it.headline, market);
        if rel <= 0.0 {
            continue;
        }
        let default_hl = if it.kind == Kind::Social { 0.5 } else { 3.0 };
        let (labels, impact, hl) = events::classify(&it.headline, default_hl);
        let age_days = (now - it.epoch).max(0) as f64 / 86_400.0;
        let weight = it.trust * it.engagement * rel * impact * 0.5f64.powf(age_days / hl);
        let polarity = events::item_polarity(&it.headline, model.as_ref(), bond);
        rows.push(ScoredItem { item: it.clone(), polarity, weight, labels });
    }
    let agg = weighted_aggregate(&rows.iter().map(|r| (r.weight, r.polarity)).collect::<Vec<_>>());
    (rows, agg)
}

/// sum(w p)/sum(w), the ratio computed by lib/sentiment.lat's `wagg` on Loom (weights and
/// polarities as signed fixed-point), with the direct computation as the fallback — the
/// same in-the-loop discipline as the original `polarity`.
pub fn weighted_aggregate(pairs: &[(f64, f64)]) -> f64 {
    if pairs.is_empty() {
        return 0.0;
    }
    let mut lit = String::from("0");
    for (w, p) in pairs.iter().rev() {
        let sf = |x: f64| {
            let m = (x.abs() * 1000.0).round() as i64;
            format!("[{} {}]", if x < 0.0 { 1 } else { 0 }, m)
        };
        lit = format!("[ [ {} {} ] {} ]", sf(*w), sf(*p), lit);
    }
    let expr = format!("(wagg {})", lit);
    match crate::latte::run_with_libs(&expr, &["std", "num", "sentiment"]) {
        Ok(v) => sf_decode(&v),
        Err(_) => {
            let (mut num, mut den) = (0.0, 0.0);
            for (w, p) in pairs {
                num += w * p;
                den += w;
            }
            if den > 0.0 { num / den } else { 0.0 }
        }
    }
}

fn sf_decode(n: &crate::knot::N) -> f64 {
    use crate::knot::Knot;
    if let Knot::Cell(h, t) = &**n {
        let sign = h.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let mag = t.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as f64 / 1000.0;
        if sign == 0 { mag } else { -mag }
    } else {
        0.0
    }
}

/// THE MARKET WIRE — what the advisors call. Ensures freshness (TTL 30 min; `force` on
/// `--live`), loads the store, scores it for the market, splits press from social. Returns
/// (press rows, press agg, social rows, social agg, provenance note); empty when the wire
/// has nothing relevant (the caller falls back to the embedded corpus).
#[allow(clippy::type_complexity)]
pub fn market_wire(market: &str, force: bool, bond: bool)
    -> (Vec<ScoredItem>, f64, Vec<ScoredItem>, f64, String)
{
    let note = ensure_fresh(1800, force);
    let now = now_epoch();
    let wire = load_wire();
    let press: Vec<Item> = wire.iter().filter(|i| i.kind == Kind::Press).cloned().collect();
    let social: Vec<Item> = wire.iter().filter(|i| i.kind == Kind::Social).cloned().collect();
    let (prows, pagg) = score_items(&press, market, bond, now);
    let (srows, sagg) = score_items(&social, market, bond, now);
    let note = format!(
        "{}; {} press + {} social item(s) relevant to {}",
        note, prows.len(), srows.len(), market
    );
    (prows, pagg, srows, sagg, note)
}

// ---------------------------------------------------------------------------
// `latte news` — the CLI surface, and the GUI's background refresher.
// ---------------------------------------------------------------------------

/// Spawn the GUI server's background wire refresher: an initial fetch when stale, then one
/// per REFRESH_SECS. Guarded exactly like detached warming: never under the test harness,
/// disabled by ORPHEUS_NEWS_AUTO=0. Failures are silent — the thread only ever adds.
pub fn spawn_refresher() {
    if cfg!(test) || std::env::var("ORPHEUS_NEWS_AUTO").map(|v| v == "0").unwrap_or(false) {
        return;
    }
    const REFRESH_SECS: u64 = 1800;
    std::thread::spawn(|| loop {
        let stale = wire_age().map(|a| a >= REFRESH_SECS as i64).unwrap_or(true);
        if stale {
            let _ = fetch_all();
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    });
}

pub fn cmd_news(args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        Some("fetch") => {
            println!("newswire — fetching every source\n");
            for n in fetch_all() {
                println!("  {}", n);
            }
            let wire = load_wire();
            println!("\n  wire: {} item(s) on the store ({})", wire.len(), wire_path().display());
        }
        Some("sources") => {
            println!("newswire sources (override with {}/sources.tsv)\n", crate::numerics::news_dir().display());
            println!("  {:<18} {:<7} {:<7} {:<6} {:<8} url", "name", "kind", "format", "trust", "markets");
            for s in sources() {
                println!(
                    "  {:<18} {:<7} {:<7} {:<6.2} {:<8} {}",
                    s.name,
                    if s.kind == Kind::Social { "social" } else { "press" },
                    match s.format { Format::Rss => "rss", Format::RedditJson => "reddit", Format::HnJson => "hn", Format::BskyJson => "bsky" },
                    s.trust, s.markets, s.url
                );
            }
        }
        Some("train") => {
            let market = args.iter().position(|a| a == "--market").and_then(|i| args.get(i + 1)).map(|s| s.as_str()).unwrap_or("btc");
            train_report(market);
        }
        Some("pulse") | Some("show") | None => {
            let market = args.iter().position(|a| a == "--market").and_then(|i| args.get(i + 1)).map(|s| s.as_str()).unwrap_or("btc");
            let force = args.iter().any(|a| a == "--live");
            pulse_report(market, force);
        }
        Some(text) if !text.starts_with('-') && args.len() == 1 && text.contains(' ') => {
            // backward compatibility: `latte news "<headline text>"` scored the text before
            // the wire existed; keep that meaning for free-text arguments
            crate::numerics::cmd_sentiment(args);
        }
        _ => {
            println!("usage: latte news [pulse [--market SYM] [--live] | fetch | train [--market SYM] | sources]");
            println!("       latte news \"<headline>\"     (score a text — same as latte sentiment)");
        }
    }
}

/// `latte news train` — fit SESTM on the wire against the market's freshest price series.
fn train_report(market: &str) {
    println!("SESTM — return-supervised sentiment (Ke–Kelly–Xiu), trained on the wire\n");
    let (closes, span, note) = match crate::marketdata::closes_market(market, false) {
        Ok(x) => x,
        Err(e) => {
            println!("  {}", e);
            return;
        }
    };
    // rebuild the dated series: the wire needs (date, close) pairs; closes_market returns
    // the span, so dates are reconstructed by day offset from the last date
    let last_ord = match crate::dates::date_ordinal(&span.1) {
        Some(o) => o,
        None => { println!("  cannot read the series span"); return; }
    };
    let series: Vec<(String, i64)> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let ord = last_ord - (closes.len() - 1 - i) as i64;
            (crate::dates::ordinal_date(ord), *c)
        })
        .collect();
    let wire = load_wire();
    let items: Vec<(String, String)> = wire
        .iter()
        .filter(|i| relevance(&i.headline, market) > 0.0)
        .map(|i| (i.date.clone(), i.headline.clone()))
        .collect();
    println!("  price series : {} ({} days)", note, closes.len());
    println!("  wire items   : {} relevant to {} (of {} on the store)", items.len(), market, wire.len());
    match events::Sestm::train(&items, &series) {
        Ok(m) => {
            let path = events::model_path(market);
            match m.save(&path) {
                Ok(()) => {
                    println!("  trained      : {} labeled items, {} .. {}", m.trained_on, m.span.0, m.span.1);
                    println!("  vocabulary   : {} sentiment-charged terms (screened at kappa={}, alpha={})",
                        m.terms.len(), 3, 0.15);
                    println!("  saved        : {}", path.display());
                    println!("\n  the wire now scores {} items with 0.5*SESTM + 0.5*(classifier+lexicon).", market);
                }
                Err(e) => println!("  trained but could not save: {}", e),
            }
        }
        Err(e) => {
            println!("  not trained  : {}", e);
            println!("\n  until then the wire scores with the trained classifier + LM lexicon");
            println!("  (event-conditioned) — the same engine, honestly labeled.");
        }
    }
}

/// `latte news [pulse]` — the wire, scored for a market, with the evidence shown.
fn pulse_report(market: &str, force: bool) {
    let bond = market == "bonds"; // the duration desk scores on the hawk/dove axis
    let (press, pagg, social, sagg, note) = market_wire(market, force, bond);
    println!("newswire — {} ({})\n", market, note);
    if press.is_empty() && social.is_empty() {
        println!("  the wire has nothing relevant yet — `latte news fetch` to pull the sources,");
        println!("  or check `latte news sources`. The advisors fall back to the embedded corpus.");
        return;
    }
    if bond {
        println!("  scoring: hawk/dove bond axis (risk-off = Treasury bid), event-conditioned");
    } else {
        let model = events::Sestm::load(&events::model_path(market));
        match &model {
            Some(m) if m.trained_on >= events::MIN_TRAIN =>
                println!("  scoring: 0.5*SESTM (trained on {} items) + 0.5*(classifier+lexicon), event-conditioned", m.trained_on),
            _ => println!("  scoring: trained classifier + LM lexicon, event-conditioned (SESTM not trained yet — `latte news train`)"),
        }
    }
    let show = |rows: &[ScoredItem], label: &str, agg: f64| {
        if rows.is_empty() {
            return;
        }
        println!("\n  -- {} --", label);
        for r in rows.iter().take(10) {
            let tags = if r.labels.is_empty() { String::new() } else { format!(" [{}]", r.labels.join(" ")) };
            let mut h = r.item.headline.clone();
            if h.len() > 90 {
                h.truncate(89);
                h.push('…');
            }
            println!("    {} {:+.2} w{:.2} {:<16} {}{}", r.item.date, r.polarity, r.weight, r.item.source, h, tags);
        }
        if rows.len() > 10 {
            println!("    … and {} more", rows.len() - 10);
        }
        println!("    weighted aggregate: {:+.2}", agg);
    };
    show(&press, "press", pagg);
    show(&social, "social pulse (half-life 12h, log-engagement weights)", sagg);
    println!("\n  `latte trade --market {}` blends these legs into the advisor automatically.", market);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_items_parse_with_dates_and_entities() {
        let xml = r#"<rss><channel>
            <item><title>Bitcoin rallies &amp; recovers</title><pubDate>Mon, 06 Jul 2026 10:00:00 +0000</pubDate></item>
            <item><title><![CDATA[ETF outflows deepen]]></title><pubDate>Sun, 05 Jul 2026 09:30:00 GMT</pubDate></item>
        </channel></rss>"#;
        let rows = parse_rss(xml, 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, "Bitcoin rallies & recovers");
        assert_eq!(rows[1].1, "ETF outflows deepen");
        assert!(rows[0].0 > rows[1].0, "RFC822 dates parsed and ordered");
    }

    #[test]
    fn atom_entries_parse_too() {
        let xml = r#"<feed><entry><title>Fed signals cuts</title><updated>2026-07-06T08:00:00Z</updated></entry></feed>"#;
        let rows = parse_rss(xml, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "Fed signals cuts");
        assert!(rows[0].0 > 0);
    }

    #[test]
    fn feed_date_survives_malformed_multibyte() {
        // a 19+ byte timestamp whose byte 19 is inside a multibyte char must not panic
        assert_eq!(parse_feed_date("2026-01-01T00:00:\u{20ac}x"), None);
        // and a normal fractional/offset date still parses
        assert!(parse_feed_date("2026-07-06T12:00:00.5+02:00").is_some());
    }

    #[test]
    fn feed_dates_cover_both_conventions() {
        let a = parse_feed_date("Mon, 06 Jul 2026 12:00:00 +0000").unwrap();
        let b = parse_feed_date("2026-07-06T12:00:00Z").unwrap();
        assert_eq!(a, b);
        let c = parse_feed_date("2026-07-06T14:00:00+02:00").unwrap();
        assert_eq!(c, b, "offset arithmetic");
        let d = parse_feed_date("2026-07-06T12:00:00.123Z").unwrap();
        assert_eq!(d, b, "fractional seconds ignored");
    }

    #[test]
    fn reddit_hn_bsky_listings_parse() {
        let reddit = r#"{"data":{"children":[
            {"kind":"t3","data":{"subreddit":"Bitcoin","title":"BTC holds the line","score":420,"created_utc":1751700000.0}},
            {"kind":"t3","data":{"subreddit":"Bitcoin","title":"Miners capitulating?","score":37,"created_utc":1751690000.0}}]}}"#;
        let r = parse_reddit(reddit);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].2, "BTC holds the line");
        assert_eq!(r[0].1, 420.0);
        assert_eq!(r[0].0, 1751700000);

        let hn = r#"{"hits":[{"created_at":"2026-07-06T01:00:00Z","title":"Bitcoin ETF flows turn","points":120,"author":"x"}]}"#;
        let h = parse_hn(hn);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].1, 120.0);

        let bsky = r#"{"posts":[{"record":{"text":"bitcoin looking strong into the CPI print this week","createdAt":"2026-07-06T02:00:00Z"},"likeCount":15}]}"#;
        let b = parse_bsky(bsky);
        assert_eq!(b.len(), 1);
        assert!(b[0].2.starts_with("bitcoin looking strong"));
    }

    #[test]
    fn json_parser_survives_adversarial_multibyte() {
        // a backslash immediately before a multibyte char (malformed JSON) must not panic;
        // parsing untrusted feed data, robustness matters more than strict correctness
        let j = "{\"title\":\"a\\\u{20ac}b crash\",\"score\":1,\"created_utc\":1751700000}";
        let r = parse_reddit(j);
        // must return something (or nothing) — the point is it does not panic
        let _ = r;
        // a lone multibyte value parses cleanly
        let j2 = "{\"title\":\"caf\u{e9} \u{20ac}100 rally\",\"score\":5,\"created_utc\":1751700000}";
        let r2 = parse_reddit(j2);
        assert_eq!(r2.len(), 1);
        assert!(r2[0].2.contains('\u{20ac}'));
    }

    #[test]
    fn json_escapes_decode() {
        let j = r#"{"title":"Fed \"pause\" \u2014 markets rally","score":5,"created_utc":1751700000}"#;
        let r = parse_reddit(j);
        assert_eq!(r.len(), 1);
        assert!(r[0].2.contains("\"pause\""));
        assert!(r[0].2.contains('—'));
    }

    #[test]
    fn engagement_is_log_scaled_and_capped() {
        assert!((engagement_factor(0.0) - 1.0).abs() < 1e-9);
        let e1k = engagement_factor(1000.0);
        assert!(e1k > 1.8 && e1k < 2.0, "a thousand upvotes ~doubles: {}", e1k);
        assert_eq!(engagement_factor(1e12), 2.0, "capped");
    }

    #[test]
    fn sources_tsv_parses_and_rejects_junk() {
        let tsv = "# comment\nmyfeed\tpress\trss\t0.8\tbtc\thttp://127.0.0.1:9/feed\nbad\tpress\trss\t7.0\tbtc\tu\n";
        let v = parse_sources(tsv);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "myfeed");
        assert!((v[0].trust - 0.8).abs() < 1e-9);
    }

    #[test]
    fn relevance_is_causal_not_lexical() {
        // direct mention: full weight
        assert_eq!(relevance("Bitcoin holds $61K", "btc"), 1.0);
        assert_eq!(relevance("Treasury yields jump after the auction", "bonds"), 1.0);
        // causal transmission WITHOUT the asset named: a Fed decision reaches both desks
        let fed = "Fed holds rates steady, signals patience";
        assert!((relevance(fed, "bonds") - 1.0).abs() < 1e-9);
        assert!((relevance(fed, "btc") - 0.7).abs() < 1e-9);
        // an auction/deficit story reaches bonds, not bitcoin (below the floor's echo)
        let auction = "Weak auction raises deficit financing concerns";
        assert!((relevance(auction, "bonds") - 1.0).abs() < 1e-9);
        assert!(relevance(auction, "btc") <= 0.35, "auction->btc = {}", relevance(auction, "btc"));
        // a hack reaches crypto and is DROPPED by the bond desk
        let hack = "Major exchange hacked, $40M drained";
        assert!(relevance(hack, "btc") > 0.8);
        assert_eq!(relevance(hack, "bonds"), 0.0);
        // sector term without the asset name
        assert!((relevance("Stablecoin rules clear the committee", "btc") - 0.8).abs() < 1e-9);
        // the fixed-income sector, named without a specific instrument, reaches the bond desk
        assert!((relevance("Credit spreads widen as high yield sells off", "bonds") - 0.8).abs() < 1e-9);
        assert!((relevance("Fixed-income funds see record inflows", "bonds") - 0.8).abs() < 1e-9);
        // sibling spillover: a story about another coin, with no recognized event class,
        // is diluted evidence about btc (the 0.4 floor)…
        assert!((relevance("Solana network congestion eases", "btc") - 0.4).abs() < 1e-9);
        // …but the event transmission is never reduced by the sibling naming: an
        // Ethereum protocol story still reaches btc at the tech channel's strength
        assert!((relevance("Ethereum upgrade ships on schedule", "btc") - 0.6).abs() < 1e-9);
        // no narrative, no mention: dropped everywhere
        assert_eq!(relevance("Local team wins the cup", "btc"), 0.0);
        assert_eq!(relevance("Local team wins the cup", "bonds"), 0.0);
    }

    #[test]
    fn scoring_weights_combine_trust_engagement_relevance_event_decay() {
        let now = 1_751_700_000i64;
        let items = vec![
            Item { epoch: now, date: "2026-07-05".into(), kind: Kind::Press, trust: 0.9,
                   engagement: 1.0, source: "t".into(), headline: "Bitcoin ETF inflows surge as institutional demand recovers".into() },
            Item { epoch: now - 86_400, date: "2026-07-04".into(), kind: Kind::Social, trust: 0.5,
                   engagement: 1.5, source: "r".into(), headline: "bitcoin pump to the moon fomo".into() },
            Item { epoch: now, date: "2026-07-05".into(), kind: Kind::Press, trust: 0.9,
                   engagement: 1.0, source: "t".into(), headline: "Sports scores from the weekend".into() },
        ];
        let (rows, agg) = score_items(&items, "btc", false, now);
        assert_eq!(rows.len(), 2, "irrelevant press item dropped");
        let etf = &rows[0];
        assert!(etf.labels.contains(&"etf-flow"));
        assert!((etf.weight - 0.9 * 1.3).abs() < 1e-6, "trust x impact, no decay at age 0: {}", etf.weight);
        let buzz = &rows[1];
        assert!(buzz.labels.contains(&"retail-buzz"));
        // a day of age at half-life 0.5d = two half-lives = /4; 0.5 trust x 1.5 eng x 0.6 impact
        let expect = 0.5 * 1.5 * 1.0 * 0.6 * 0.25;
        assert!((buzz.weight - expect).abs() < 1e-6, "buzz decays fast: {} vs {}", buzz.weight, expect);
        assert!(etf.polarity > 0.0);
        assert!(agg.abs() <= 1.0);
    }

    #[test]
    fn weighted_aggregate_matches_direct_arithmetic() {
        let pairs = vec![(1.0, 0.5), (0.5, -0.2), (0.25, 0.8)];
        let (mut num, mut den) = (0.0, 0.0);
        for (w, p) in &pairs {
            num += w * p;
            den += w;
        }
        let direct = num / den;
        let agg = weighted_aggregate(&pairs);
        assert!((agg - direct).abs() < 0.01, "loom {} vs direct {}", agg, direct);
        assert_eq!(weighted_aggregate(&[]), 0.0);
    }

    #[test]
    fn wire_roundtrip_dedup_and_sanitize() {
        // point the cache at a temp dir for this test
        let dir = std::env::temp_dir().join(format!("orpheus-wire-test-{}", std::process::id()));
        std::env::set_var("ORPHEUS_CACHE", &dir);
        let now = now_epoch();
        let items = vec![
            Item { epoch: now, date: "2026-07-06".into(), kind: Kind::Press, trust: 0.9,
                   engagement: 1.0, source: "a".into(), headline: "Tabs\tand\nnewlines collapse".into() },
        ];
        save_wire(&items);
        let back = load_wire();
        assert_eq!(back.len(), 1);
        assert!(!back[0].headline.contains('\t') && !back[0].headline.contains('\n'));
        assert_eq!(item_key("Bitcoin Rallies!"), item_key("bitcoin  rallies"), "normalized dedup key");
        std::env::remove_var("ORPHEUS_CACHE");
        let _ = std::fs::remove_dir_all(dir);
    }
}
