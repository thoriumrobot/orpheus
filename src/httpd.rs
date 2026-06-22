//! One HTTP/1.1 server core, shared by every Orpheus HTTP surface.
//!
//! Historically the Hymn web/GUI server (`serve.rs`) and the Anvil artifact registry (`registry.rs`)
//! each carried their own copy of the wire protocol. This module is the single implementation both
//! now use: it owns the accept loop, persistent connections (keep-alive), request parsing, response
//! writing, `HEAD` handling, ETag/conditional-GET, byte ranges, `Date`/`Server` headers, and access
//! logging — and leaves *routing* to a pluggable [`Handler`]. A surface is then just a `Handler`
//! impl plus a one-line `httpd::serve(addr, handler)`.
//!
//! Different surfaces have different needs, so the parts that genuinely vary are per-handler knobs
//! rather than forks of the server: [`Handler::max_body`] (the GUI caps uploads to guard against
//! floods; the registry must accept multi-megabyte binaries) and [`Handler::read_timeout`]. The
//! `anvild` compile daemon deliberately does *not* live here — it speaks a line-oriented verb
//! protocol over a Unix socket, not HTTP, and pretending otherwise would be a worse abstraction.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::sha3;

// ----- request / response types --------------------------------------------

pub struct Request {
    pub method: String,
    pub path: String, // percent-decoded, query stripped
    pub version: String,
    pub headers: HashMap<String, String>, // lowercased keys
    pub keep_alive: bool,
    pub body: Vec<u8>,
    pub query: String, // raw query string (after '?')
}

impl Request {
    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers.get(k).map(|s| s.as_str())
    }
    pub fn is_head(&self) -> bool {
        self.method == "HEAD"
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub is_body_suppressed: bool, // 304: never send a body
}

impl Response {
    /// A response with standard base headers (`Date`/`Server`/`Content-Type`/`Cache-Control`) and a
    /// `Content-Length` for `body`.
    pub fn bytes(status: u16, ctype: &str, body: Vec<u8>) -> Response {
        let mut headers = base_headers(ctype, "no-cache");
        headers.push(("Content-Length".into(), body.len().to_string()));
        Response { status, headers, body, is_body_suppressed: false }
    }
    /// A `text/plain` response.
    pub fn text(status: u16, body: &str) -> Response {
        Response::bytes(status, "text/plain; charset=utf-8", body.as_bytes().to_vec())
    }
    /// Append a header (builder style).
    pub fn with_header(mut self, k: &str, v: &str) -> Response {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }
}

// ----- the pluggable routing seam ------------------------------------------

/// A routing surface over the shared core. Implementors decide what each request returns; the core
/// handles everything below the request/response line.
pub trait Handler: Send + Sync {
    fn handle(&self, req: &Request) -> Response;
    /// Largest request body to accept (bytes). Default guards against floods.
    fn max_body(&self) -> usize {
        8 * 1024 * 1024
    }
    /// Per-connection read timeout.
    fn read_timeout(&self) -> Duration {
        Duration::from_secs(15)
    }
    /// Maximum number of connections handled concurrently. Excess connections are answered `503`
    /// and closed, bounding thread/memory use under load instead of spawning without limit.
    fn max_connections(&self) -> usize {
        1024
    }
    /// Whether to emit an access log line per request.
    fn log(&self) -> bool {
        true
    }
}

// ----- accept loop ----------------------------------------------------------

/// Bind `listen` and serve forever, dispatching every request to `handler`. Concurrency is bounded
/// by [`Handler::max_connections`]: excess connections receive `503` and are closed, so the server
/// degrades gracefully under load rather than spawning threads without limit. Returns only on bind
/// failure.
pub fn serve<H: Handler + 'static>(listen: &str, handler: Arc<H>) -> std::io::Result<()> {
    let listener = TcpListener::bind(listen)?;
    let active = Arc::new(AtomicUsize::new(0));
    let max = handler.max_connections().max(1);
    for conn in listener.incoming() {
        if let Ok(stream) = conn {
            if active.load(Ordering::Acquire) >= max {
                reject_overloaded(stream);
                continue;
            }
            active.fetch_add(1, Ordering::AcqRel);
            let h = handler.clone();
            let a = active.clone();
            std::thread::spawn(move || {
                let _ = handle_conn(stream, h);
                a.fetch_sub(1, Ordering::AcqRel);
            });
        }
    }
    Ok(())
}

/// Politely turn away a connection when at capacity: a short-timeout `503`, then close.
fn reject_overloaded(mut stream: TcpStream) {
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let _ = write_error(&mut stream, 503);
}

fn handle_conn<H: Handler>(stream: TcpStream, handler: Arc<H>) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(handler.read_timeout())).ok();
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "?".into());
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let req = match parse_request(&mut reader, handler.max_body())? {
            Incoming::Eof => break, // client closed / idle timeout
            Incoming::Error(code) => {
                // Malformed or abusive request: the core answers directly and closes, so handlers
                // only ever see well-formed requests.
                write_error(&mut writer, code)?;
                break;
            }
            Incoming::Request(r) => r,
        };
        let keep = req.keep_alive;
        let resp = handler.handle(&req);
        if handler.log() {
            log_line(&peer, &req, &resp);
        }
        write_response(&mut writer, &req, &resp)?;
        if !keep {
            break;
        }
    }
    Ok(())
}

/// Largest acceptable request line or single header line (bytes). Guards against a client streaming
/// an unbounded line with no terminator (memory exhaustion / slowloris).
const MAX_LINE: usize = 16 * 1024;

enum Line {
    Eof,
    Ok(String),
    TooLong,
}

/// Read one CRLF-terminated line, but never buffer more than `max` bytes — the defense the stock
/// `read_line`/`read_until` lack.
fn read_line_capped<R: BufRead>(reader: &mut R, max: usize) -> std::io::Result<Line> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = reader.read(&mut byte)?;
        if n == 0 {
            return Ok(if buf.is_empty() { Line::Eof } else { Line::Ok(String::from_utf8_lossy(&buf).into_owned()) });
        }
        match byte[0] {
            b'\n' => return Ok(Line::Ok(String::from_utf8_lossy(&buf).into_owned())),
            b'\r' => {} // ignore; CRLF or bare CR both fine
            b => {
                buf.push(b);
                if buf.len() > max {
                    return Ok(Line::TooLong);
                }
            }
        }
    }
}

enum Incoming {
    Eof,
    Request(Request),
    Error(u16),
}

/// Read one request. Protocol violations and abusive sizes become an [`Incoming::Error`] with the
/// right status (`400`/`413`/`414`/`431`) instead of being silently tolerated.
fn parse_request<R: BufRead>(reader: &mut R, max_body: usize) -> std::io::Result<Incoming> {
    let line = match read_line_capped(reader, MAX_LINE)? {
        Line::Eof => return Ok(Incoming::Eof),
        Line::TooLong => return Ok(Incoming::Error(414)), // URI Too Long
        Line::Ok(s) => s,
    };
    let mut parts = line.trim_end().split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let version = parts.next().unwrap_or("HTTP/1.0").to_string();
    if method.is_empty() {
        return Ok(Incoming::Error(400));
    }
    let mut headers = HashMap::new();
    loop {
        let h = match read_line_capped(reader, MAX_LINE)? {
            Line::Eof => break,
            Line::TooLong => return Ok(Incoming::Error(431)), // Request Header Fields Too Large
            Line::Ok(s) => s,
        };
        let t = h.trim_end();
        if t.is_empty() {
            break; // end of headers
        }
        if let Some((k, v)) = t.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
        if headers.len() > 100 {
            return Ok(Incoming::Error(431)); // header flood
        }
    }
    let conn = headers.get("connection").map(|s| s.to_ascii_lowercase());
    let keep_alive = if version == "HTTP/1.1" {
        conn.as_deref() != Some("close")
    } else {
        conn.as_deref() == Some("keep-alive")
    };
    let clen = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    if clen > max_body {
        return Ok(Incoming::Error(413)); // Payload Too Large
    }
    let mut body = Vec::new();
    if clen > 0 {
        body.resize(clen, 0);
        reader.read_exact(&mut body)?;
    }
    let mut tparts = target.splitn(2, '?');
    let raw_path = tparts.next().unwrap_or("/");
    let query = tparts.next().unwrap_or("").to_string();
    let path = percent_decode(raw_path);
    Ok(Incoming::Request(Request { method, path, version, headers, keep_alive, body, query }))
}

/// A self-contained error reply that always closes the connection.
fn write_error(w: &mut TcpStream, code: u16) -> std::io::Result<()> {
    let body = format!("{} {}", code, reason(code));
    let head = format!(
        "HTTP/1.1 {} {}\r\nServer: Hymn\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        code,
        reason(code),
        body.len()
    );
    w.write_all(head.as_bytes())?;
    w.write_all(body.as_bytes())?;
    w.flush()
}

fn write_response(w: &mut TcpStream, req: &Request, resp: &Response) -> std::io::Result<()> {
    let reason = reason(resp.status);
    let mut head = format!("HTTP/1.1 {} {}\r\n", resp.status, reason);
    for (k, v) in &resp.headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str(if req.keep_alive { "Connection: keep-alive\r\n" } else { "Connection: close\r\n" });
    head.push_str("\r\n");
    w.write_all(head.as_bytes())?;
    let send_body = req.method != "HEAD" && !resp.is_body_suppressed;
    if send_body {
        w.write_all(&resp.body)?;
    }
    w.flush()
}

fn reason(s: u16) -> &'static str {
    match s {
        200 => "OK",
        206 => "Partial Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        416 => "Range Not Satisfiable",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

fn log_line(peer: &str, req: &Request, resp: &Response) {
    println!("Hymn {} \"{} {} {}\" {} {}B", peer, req.method, req.path, req.version, resp.status, resp.body.len());
}

// ----- reusable response helpers -------------------------------------------

pub fn base_headers(ctype: &str, cache: &str) -> Vec<(String, String)> {
    vec![
        ("Date".into(), httpdate(now_secs())),
        ("Server".into(), "Hymn".into()),
        ("Content-Type".into(), ctype.to_string()),
        ("Cache-Control".into(), cache.to_string()),
    ]
}

/// A response with base headers + `Content-Length` (no ETag/validators). Kept for call sites that
/// want the simplest possible reply.
pub fn simple(status: u16, ctype: &str, body: Vec<u8>) -> Response {
    Response::bytes(status, ctype, body)
}

pub fn etag_of(body: &[u8]) -> String {
    let d = sha3::sha3_256(body);
    format!("\"{}\"", &sha3::hex(&d)[..16])
}

pub fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let spec = h.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None; // we serve a single range only
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = if a.is_empty() {
        let suffix: u64 = b.trim().parse().ok()?;
        if suffix == 0 {
            return None;
        }
        (len.saturating_sub(suffix), len - 1)
    } else {
        let start: u64 = a.trim().parse().ok()?;
        let end: u64 = if b.trim().is_empty() { len - 1 } else { b.trim().parse().ok()? };
        (start, end.min(len - 1))
    };
    if start > end || start >= len {
        return None;
    }
    Some((start, end))
}

pub fn content_type(ext: &str) -> Option<(&'static str, bool)> {
    Some(match ext {
        "css" => ("text/css; charset=utf-8", false),
        "txt" | "sca" | "lat" => ("text/plain; charset=utf-8", false),
        "md" => ("text/markdown; charset=utf-8", false),
        "html" | "htm" => ("text/html; charset=utf-8", false),
        "js" => ("text/javascript; charset=utf-8", false),
        "json" => ("application/json; charset=utf-8", false),
        "svg" => ("image/svg+xml; charset=utf-8", false),
        "png" => ("image/png", true),
        "ico" => ("image/x-icon", true),
        "woff2" => ("font/woff2", true),
        "woff" => ("font/woff", true),
        "ttf" => ("font/ttf", true),
        "otf" => ("font/otf", true),
        _ => return None,
    })
}

// ----- small helpers --------------------------------------------------------

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hexval(b[i + 1]), hexval(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn httpdate(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (hh, mi, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let wday = (((days % 7) + 4) % 7 + 7) % 7; // 1970-01-01 = Thursday(4); 0=Sun
    // civil_from_days (Howard Hinnant)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        WD[wday as usize], d, MO[(m - 1) as usize], year, hh, mi, ss
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &[u8], max_body: usize) -> Incoming {
        let mut r = std::io::BufReader::new(raw);
        parse_request(&mut r, max_body).unwrap()
    }

    #[test]
    fn parses_request_line_and_headers() {
        let raw = b"GET /a/b?x=1 HTTP/1.1\r\nHost: h\r\nContent-Length: 3\r\n\r\nabc";
        match parse(raw, 1024) {
            Incoming::Request(req) => {
                assert_eq!(req.method, "GET");
                assert_eq!(req.path, "/a/b");
                assert_eq!(req.query, "x=1");
                assert_eq!(req.body, b"abc");
                assert!(req.keep_alive);
            }
            _ => panic!("expected a parsed request"),
        }
    }

    #[test]
    fn oversized_body_is_rejected() {
        // Declared length exceeds the cap → 413 rather than a silently-empty body.
        let raw = b"PUT /x HTTP/1.1\r\nContent-Length: 100\r\n\r\n";
        assert!(matches!(parse(raw, 10), Incoming::Error(413)));
    }

    #[test]
    fn oversized_request_line_and_headers_are_rejected() {
        // A request line longer than MAX_LINE with no terminator → 414, not unbounded buffering.
        let mut huge = b"GET /".to_vec();
        huge.extend(std::iter::repeat(b'a').take(MAX_LINE + 10));
        assert!(matches!(parse(&huge, 1024), Incoming::Error(414)));
        // A single header line over the cap → 431.
        let mut h = b"GET / HTTP/1.1\r\nX: ".to_vec();
        h.extend(std::iter::repeat(b'a').take(MAX_LINE + 10));
        assert!(matches!(parse(&h, 1024), Incoming::Error(431)));
    }

    #[test]
    fn empty_input_is_eof() {
        assert!(matches!(parse(b"", 1024), Incoming::Eof));
    }

    #[test]
    fn reason_and_dates() {
        assert_eq!(reason(401), "Unauthorized");
        assert_eq!(reason(413), "Payload Too Large");
        assert_eq!(reason(431), "Request Header Fields Too Large");
        assert_eq!(reason(503), "Service Unavailable");
        assert!(httpdate(1_000_000).ends_with("GMT"));
        assert_eq!(percent_decode("a%20b"), "a b");
    }
}
