//! Hymn — the web server of the Orpheus environment.
//!
//! A small but fully-featured HTTP/1.1 server. It hosts a directory of Facet pages
//! (rendered on each request, so `SCArs.evolve` and other tools fill in content) and
//! serves static assets — stylesheets, text, fonts — with correct MIME types and
//! exact bytes, so stacked diacritics like `ō̃`/`ū̃` render from an `@font-face`.
//!
//! HTTP features: HTTP/1.1 persistent connections (keep-alive), header parsing,
//! conditional GET (content `ETag` via SHA3 + `If-None-Match` → 304; `Last-Modified`
//! + `If-Modified-Since`), byte `Range` requests (`206`/`416`, `Accept-Ranges`),
//! `Date`/`Last-Modified` headers, `HEAD`, access logging, read timeouts, and proper
//! `400`/`404`/`405` handling. Rendering is pure and holds no shared mutable state, so
//! concurrent requests use SCArs without races.

use crate::knot::{Knot, N};
use crate::{check, facet, latte, sca, sha3};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A shared, editable document backing the WYSIWYG editor: a Mocha `editor` app
/// (the document model, in Latte) on a persistent Node.
pub struct Editor {
    node: std::sync::Mutex<crate::net::Node>,
    q: crate::mocha::Mocha,
}
pub type EditorHandle = std::sync::Arc<Editor>;

impl Editor {
    pub fn new(node: crate::net::Node, q: crate::mocha::Mocha) -> EditorHandle {
        std::sync::Arc::new(Editor { node: std::sync::Mutex::new(node), q })
    }
}

/// The shared chess game backing the board GUI: a Mocha `chessgame` app on a networked
/// Node. Moves are gossiped to peers, so two GUI servers that peer with one another share
/// one converging board — the "play against another user on a connected machine" mode.
pub struct Chess {
    node: crate::net::NodeHandle,
    peers: crate::net::Peers,
    q: crate::mocha::Mocha,
}
pub type ChessHandle = std::sync::Arc<Chess>;
impl Chess {
    pub fn new(node: crate::net::NodeHandle, peers: crate::net::Peers, q: crate::mocha::Mocha) -> ChessHandle {
        std::sync::Arc::new(Chess { node, peers, q })
    }
}

pub fn serve(listen: &str, root: &str) {
    serve_with(listen, root, None, None);
}

/// Serve the GUI: the static site/Facet pages plus the live editor API, backed by `editor`,
/// and the chess board, backed by `chess`.
pub fn serve_gui(listen: &str, root: &str, editor: EditorHandle, chess: Option<ChessHandle>) {
    serve_with(listen, root, Some(editor), chess);
}

fn serve_with(listen: &str, root: &str, editor: Option<EditorHandle>, chess: Option<ChessHandle>) {
    let listener = match TcpListener::bind(listen) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Hymn: cannot bind {}: {}", listen, e);
            return;
        }
    };
    println!("Hymn — Orpheus web server (HTTP/1.1)");
    println!("  hosting '{}' at http://{}/", root, listen);
    if editor.is_some() {
        println!("  WYSIWYG editor at  http://{}/editor   (live Facet preview, save/load)", listen);
    }
    println!("  keep-alive · ETag/304 · Range/206 · fonts · SCArs-powered Facet pages");
    for stream in listener.incoming().flatten() {
        let root = root.to_string();
        let editor = editor.clone();
        let chess = chess.clone();
        std::thread::spawn(move || {
            let _ = handle_conn(stream, &root, editor, chess);
        });
    }
}

// ----- request / response model ---------------------------------------------
struct Request {
    method: String,
    path: String, // decoded, query stripped
    version: String,
    headers: HashMap<String, String>, // lowercased keys
    keep_alive: bool,
    body: Vec<u8>,
    query: String, // raw query string (after '?')
}

impl Request {
    fn header(&self, k: &str) -> Option<&str> {
        self.headers.get(k).map(|s| s.as_str())
    }
}

struct Resource {
    body: Vec<u8>,
    ctype: String,
    cacheable: bool,
    last_modified: Option<u64>, // unix seconds
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    is_body_suppressed: bool, // 304: never send a body
}

// ----- connection loop ------------------------------------------------------
fn handle_conn(stream: TcpStream, root: &str, editor: Option<EditorHandle>, chess: Option<ChessHandle>) -> std::io::Result<()> {
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "?".into());
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let req = match parse_request(&mut reader)? {
            Some(r) => r,
            None => break, // client closed / idle timeout
        };
        let keep = req.keep_alive;
        let resp = if req.path.starts_with("/api/") {
            api_handle(&req, &editor, &chess, root)
        } else if editor.is_some() && req.path == "/" {
            // the GUI's home is the System console
            respond_for(&req, resolve(root, "/system"))
        } else {
            respond_for(&req, resolve(root, &req.path))
        };
        log_line(&peer, &req, &resp);
        write_response(&mut writer, &req, &resp)?;
        if !keep {
            break;
        }
    }
    Ok(())
}

/// Read one request (request line + headers). Returns None at clean EOF.
fn parse_request<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Request>> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    let mut parts = line.trim_end().split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let version = parts.next().unwrap_or("HTTP/1.0").to_string();
    if method.is_empty() {
        return Ok(Some(bad()));
    }
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        let t = h.trim_end();
        if t.is_empty() {
            break; // end of headers
        }
        if let Some((k, v)) = t.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
        if headers.len() > 100 {
            break; // header flood guard
        }
    }
    let conn = headers.get("connection").map(|s| s.to_ascii_lowercase());
    let keep_alive = if version == "HTTP/1.1" {
        conn.as_deref() != Some("close")
    } else {
        conn.as_deref() == Some("keep-alive")
    };
    let clen = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let mut body = Vec::new();
    if clen > 0 && clen < 8 * 1024 * 1024 {
        body.resize(clen, 0);
        reader.read_exact(&mut body)?;
    }
    let mut tparts = target.splitn(2, '?');
    let raw_path = tparts.next().unwrap_or("/");
    let query = tparts.next().unwrap_or("").to_string();
    let path = percent_decode(raw_path);
    Ok(Some(Request { method, path, version, headers, keep_alive, body, query }))
}

fn bad() -> Request {
    Request {
        method: "BAD".into(),
        path: "/".into(),
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        keep_alive: false,
        body: Vec::new(),
        query: String::new(),
    }
}

// ----- routing --------------------------------------------------------------
fn resolve(root: &str, path: &str) -> Option<Resource> {
    if path.contains("..") {
        return None;
    }
    let rel = path.trim_start_matches('/');
    let base = Path::new(root);
    let file = if path.is_empty() || path == "/" || path.ends_with('/') {
        base.join(rel).join("index.facet")
    } else {
        base.join(rel)
    };
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");

    // a Facet page: render it (dynamic, no Last-Modified)
    if ext == "facet" || (ext.is_empty() && file.with_extension("facet").exists()) {
        let f = if ext == "facet" { file } else { file.with_extension("facet") };
        let src = fs::read_to_string(&f).ok()?;
        let body = match facet::render(&src) {
            Ok(html) => html.into_bytes(),
            Err(e) => {
                return Some(Resource {
                    body: format!(
                        "<!doctype html><meta charset=utf-8><h1>Facet render error</h1><pre>{}</pre>",
                        html_escape(&e)
                    )
                    .into_bytes(),
                    ctype: "text/html; charset=utf-8".into(),
                    cacheable: false,
                    last_modified: None,
                });
            }
        };
        return Some(Resource { body, ctype: "text/html; charset=utf-8".into(), cacheable: false, last_modified: None });
    }

    // extensionless path that names a static .html (e.g. /editor -> editor.html)
    let file = if ext.is_empty() && file.with_extension("html").exists() {
        file.with_extension("html")
    } else {
        file
    };
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");

    // a static asset
    let (ctype, cacheable) = content_type(ext)?;
    let meta = fs::metadata(&file).ok()?;
    let last_modified = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let body = fs::read(&file).ok()?;
    Some(Resource { body, ctype: ctype.to_string(), cacheable, last_modified })
}

fn content_type(ext: &str) -> Option<(&'static str, bool)> {
    Some(match ext {
        "css" => ("text/css; charset=utf-8", false),
        "txt" | "sca" | "lat" => ("text/plain; charset=utf-8", false),
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

// ----- the WYSIWYG editor API (dynamic) -------------------------------------
/// Handle `/api/*`: live Facet rendering and document save/load for the editor.
fn api_handle(req: &Request, editor: &Option<EditorHandle>, chess: &Option<ChessHandle>, root: &str) -> Response {
    match (req.method.as_str(), req.path.as_str()) {
        // live preview: render submitted Facet source to HTML (errors shown inline)
        ("POST", "/api/render") => {
            let src = String::from_utf8_lossy(&req.body);
            let html = match facet::render(&src) {
                Ok(h) => h,
                Err(e) => format!("<pre style=\"color:#b00020\">Facet error: {}</pre>", html_escape(&e)),
            };
            simple(200, "text/html; charset=utf-8", html.into_bytes())
        }
        // save the whole document into the Latte `editor` app (durable, syncable)
        ("POST", "/api/save") => match editor {
            Some(ed) => {
                let text = String::from_utf8_lossy(&req.body).to_string();
                let action = crate::knot::cell(crate::knot::cord("set"), crate::knot::cord(&text));
                ed.node.lock().unwrap().local_action(action);
                simple(200, "text/plain; charset=utf-8", b"saved".to_vec())
            }
            None => simple(503, "text/plain; charset=utf-8", b"no editor".to_vec()),
        },
        // load the saved document text
        ("GET", "/api/load") => match editor {
            Some(ed) => {
                let n = ed.node.lock().unwrap();
                let st = n.state().unwrap_or_else(|_| crate::knot::num(0));
                let query = crate::knot::cell(crate::knot::cord("text"), crate::knot::num(0));
                let text = ed
                    .q
                    .peek(&query, &st)
                    .ok()
                    .and_then(|v| v.as_atom().map(|a| String::from_utf8_lossy(a.bytes_le()).into_owned()))
                    .unwrap_or_default();
                simple(200, "text/plain; charset=utf-8", text.into_bytes())
            }
            None => simple(200, "text/plain; charset=utf-8", Vec::new()),
        },
        // list the editable Facet page files in the site root
        ("GET", "/api/files") => {
            let mut names: Vec<String> = std::fs::read_dir(root)
                .map(|rd| {
                    rd.flatten()
                        .filter_map(|e| e.file_name().into_string().ok())
                        .filter(|n| n.ends_with(".facet"))
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            simple(200, "text/plain; charset=utf-8", names.join("\n").into_bytes())
        }
        // load / save a single Facet page file (Unicode-safe: raw bytes)
        ("GET", "/api/file") => match safe_facet_path(root, query_param(&req.query, "path").as_deref()) {
            Some(path) => match std::fs::read(&path) {
                Ok(bytes) => simple(200, "text/plain; charset=utf-8", bytes),
                Err(_) => simple(404, "text/plain; charset=utf-8", b"no such file".to_vec()),
            },
            None => simple(400, "text/plain; charset=utf-8", b"bad path".to_vec()),
        },
        ("POST", "/api/file") => match safe_facet_path(root, query_param(&req.query, "path").as_deref()) {
            Some(path) => match std::fs::write(&path, &req.body) {
                Ok(_) => simple(200, "text/plain; charset=utf-8", b"saved".to_vec()),
                Err(e) => simple(500, "text/plain; charset=utf-8", format!("write failed: {}", e).into_bytes()),
            },
            None => simple(400, "text/plain; charset=utf-8", b"bad path".to_vec()),
        },
        // unified tool runner (Oberon-style "execute this command"): eval / type / sca
        ("POST", "/api/run") => {
            let cmd = String::from_utf8_lossy(&req.body);
            simple(200, "text/plain; charset=utf-8", run_tool(cmd.trim()).into_bytes())
        }
        // Oberon-style in-system compilation: compile a Latte module and load it.
        ("POST", "/api/compile") => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            let msg = match crate::latte::compile_and_register(&body) {
                Ok(m) => m,
                Err(e) => format!("compile error: {}", e),
            };
            simple(200, "text/plain; charset=utf-8", msg.into_bytes())
        }
        // register a library at run time: body = "NAME\n<latte source>"
        ("POST", "/api/lib") => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            match body.split_once('\n') {
                Some((name, src)) if !name.trim().is_empty() => {
                    crate::latte::register_runtime_lib(name.trim(), src);
                    simple(
                        200,
                        "text/plain; charset=utf-8",
                        format!("registered library '{}' (now importable; runtime libs: {})",
                            name.trim(),
                            crate::latte::runtime_lib_names().join(", "))
                        .into_bytes(),
                    )
                }
                _ => simple(400, "text/plain; charset=utf-8", b"usage: first line = name, rest = source".to_vec()),
            }
        }
        // ---- the system's own source (Oberon self-hosting) ----------------
        // List the modules whose source can be opened/edited: built-ins, runtime-compiled,
        // and any extra `*.lat` files in the library directory.
        ("GET", "/api/sources") => {
            let mut names: Vec<String> = crate::latte::builtin_lib_names();
            names.extend(crate::latte::runtime_lib_names());
            if let Some(dir) = lib_dir(root) {
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.extension().and_then(|x| x.to_str()) == Some("lat") {
                            if let Some(stem) = p.file_stem().and_then(|x| x.to_str()) {
                                names.push(stem.to_string());
                            }
                        }
                    }
                }
            }
            names.sort();
            names.dedup();
            simple(200, "text/plain; charset=utf-8", names.join("\n").into_bytes())
        }
        // Read a module's current source: runtime override, else built-in, else file on disk.
        ("GET", "/api/source") => match safe_lib_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                if let Some(src) = crate::latte::library_source(&name) {
                    simple(200, "text/plain; charset=utf-8", src.into_bytes())
                } else if let Some(p) = lib_dir(root).map(|d| d.join(format!("{}.lat", name))) {
                    match std::fs::read(&p) {
                        Ok(b) => simple(200, "text/plain; charset=utf-8", b),
                        Err(_) => simple(404, "text/plain; charset=utf-8", b"no such module".to_vec()),
                    }
                } else {
                    simple(404, "text/plain; charset=utf-8", b"no such module".to_vec())
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad module name".to_vec()),
        },
        // Compile a module's source into the running system and persist it to disk if possible.
        // This is the GUI's edit -> compile -> run loop applied to the system's own modules.
        ("POST", "/api/source") => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            match crate::latte::compile_and_register(&body) {
                Ok(mut msg) => {
                    // best-effort persist to <libdir>/NAME.lat (does not affect the live load)
                    if let Some(name) = find_core_name(&body) {
                        if let Some(p) = lib_dir(root).map(|d| d.join(format!("{}.lat", name))) {
                            match std::fs::write(&p, body.as_bytes()) {
                                Ok(_) => msg.push_str(" — saved to disk"),
                                Err(_) => msg.push_str(" — (in-memory only; library dir not writable)"),
                            }
                        }
                    }
                    simple(200, "text/plain; charset=utf-8", msg.into_bytes())
                }
                Err(e) => simple(200, "text/plain; charset=utf-8", format!("compile error: {}", e).into_bytes()),
            }
        }
        // ---- the documentation, served into the GUI ----------------------
        ("GET", "/api/docs") => {
            let mut names: Vec<String> = Vec::new();
            if let Ok(rd) = std::fs::read_dir(docs_dir(root)) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("md") {
                        if let Some(stem) = p.file_stem().and_then(|x| x.to_str()) {
                            names.push(stem.to_string());
                        }
                    }
                }
            }
            names.sort();
            // a friendly default order: language references first
            names.sort_by_key(|n| match n.as_str() {
                "latte-language" => 0,
                "facet-language" => 1,
                "scars-sound-changes" => 2,
                "interaction-nets" => 3,
                "adding-libraries" => 4,
                _ => 9,
            });
            simple(200, "text/plain; charset=utf-8", names.join("\n").into_bytes())
        }
        ("GET", "/api/doc") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let p = docs_dir(root).join(format!("{}.md", name));
                match std::fs::read(&p) {
                    Ok(b) => simple(200, "text/markdown; charset=utf-8", b),
                    Err(_) => simple(404, "text/plain; charset=utf-8", b"no such doc".to_vec()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad doc name".to_vec()),
        },
        // economic planning: body = "iters demand_steel demand_grain" (demand in thousandths)
        ("POST", "/api/plan") => {
            let body = String::from_utf8_lossy(&req.body);
            let nums: Vec<u128> = body.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            let iters = nums.first().copied().unwrap_or(60) as u64;
            let ds = nums.get(1).copied().unwrap_or(0);
            let dg = nums.get(2).copied().unwrap_or(1000);
            simple(200, "text/plain; charset=utf-8", crate::plan::plan_report(ds, dg, iters).into_bytes())
        }
        // data visualization: body = "bar|line|scatter n n n ..." -> SVG
        ("POST", "/api/plot") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut it = body.split_whitespace();
            let kinds = ["bar", "line", "scatter"];
            let first = it.clone().next().unwrap_or("");
            let kind = if kinds.contains(&first) { it.next(); first } else { "bar" };
            let vals: Vec<u128> = it.filter_map(|t| t.parse().ok()).collect();
            simple(200, "image/svg+xml; charset=utf-8", crate::viz::render_chart(kind, &vals).into_bytes())
        }
        // ---- the chess board (graphical game frontend) --------------------
        // Body is one line: `state` | `new` | `move FROM TO` | `ai greedy|ml`.
        // The game lives in the chess Mocha node, so `move` events gossip to any peered
        // machine — that is the "play against another user on a connected machine" mode.
        ("POST", "/api/chess") => match chess {
            None => simple(503, "text/plain; charset=utf-8", b"chess not enabled".to_vec()),
            Some(ch) => {
                let body = String::from_utf8_lossy(&req.body);
                let mut it = body.split_whitespace();
                let verb = it.next().unwrap_or("state");
                match verb {
                    "new" => {
                        crate::net::submit(&ch.node, &ch.peers, crate::knot::cell(crate::knot::cord("new"), crate::knot::num(0)));
                    }
                    "move" => {
                        if let (Some(f), Some(t)) = (it.next().and_then(|s| s.parse::<u128>().ok()), it.next().and_then(|s| s.parse::<u128>().ok())) {
                            let mv = crate::knot::cell(crate::knot::num(f), crate::knot::cell(crate::knot::num(t), crate::knot::num(0)));
                            crate::net::submit(&ch.node, &ch.peers, crate::knot::cell(crate::knot::cord("move"), mv));
                        }
                    }
                    "ai" => {
                        let ml = it.next() == Some("ml");
                        let st = ch.node.lock().unwrap().state().unwrap_or_else(|_| crate::knot::num(0));
                        // peek %state so an empty node resolves to the opening position
                        let st = ch.q.peek(&chess_query("state"), &st).unwrap_or(st);
                        if let Some(mv) = crate::game::ai_move(&st, ml) {
                            crate::net::submit(&ch.node, &ch.peers, crate::knot::cell(crate::knot::cord("move"), mv));
                        }
                    }
                    _ => {} // "state" or anything else: just report
                }
                simple(200, "application/json; charset=utf-8", chess_json(ch).into_bytes())
            }
        },
        _ => simple(404, "text/plain; charset=utf-8", b"404 (unknown /api route)".to_vec()),
    }
}

/// Build the `[tag 0]` query noun for a chessgame peek.
fn chess_query(tag: &str) -> N {
    crate::knot::cell(crate::knot::cord(tag), crate::knot::num(0))
}

/// Render the current chess game as JSON for the board UI. The position is read from the
/// (gossiped) node, but legality and status are computed on the fast unbounded engine —
/// the node's peek is fuel-bounded and cannot complete the heavy move generation.
fn chess_json(ch: &Chess) -> String {
    let raw = ch.node.lock().unwrap().state().unwrap_or_else(|_| crate::knot::num(0));
    // peek %state normalises an empty node to the opening position
    let st = ch.q.peek(&chess_query("state"), &raw).unwrap_or(raw);
    let (board, side) = crate::game::board_side(&st);
    let status = crate::game::status_of(&st);
    let legal = crate::game::legal_moves(&st);
    let board_s = board.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    let legal_s = legal.iter().map(|(f, t)| format!("[{},{}]", f, t)).collect::<Vec<_>>().join(",");
    format!(
        "{{\"board\":[{}],\"side\":{},\"status\":{},\"legal\":[{}]}}",
        board_s, side, status, legal_s
    )
}

/// Run a one-line tool command (the Oberon-style "execute this command"):
/// `eval <expr>`, `type <expr>`, or `sca <words>`.
fn run_tool(cmd: &str) -> String {
    if cmd.is_empty() {
        return "commands:\n  eval <expr>   run a Latte expression (std · mold · num · tensor · plan · ml · tool)\n  type <expr>   infer an expression's type\n  sca  <words>  evolve Solar words into Heart Speech (SCArs)\n  icomb         reduce interaction combinators (Lafont γ/δ/ε)\n  libs          list the libraries (modules) loaded in the running system\n  Module.cmd a  run arm `cmd` of a loaded Latte module on the argument(s) `a`"
            .into();
    }
    let (head, rest) = match cmd.split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim()),
        None => (cmd, ""),
    };
    match head {
        "eval" => eval_expr(rest),
        "type" => match latte::parse(rest) {
            Ok(ast) => match check::check(&ast) {
                Ok(ty) => format!("{} : {}", rest, ty.show()),
                Err(e) => format!("type error: {}", e),
            },
            Err(e) => format!("parse error: {}", e),
        },
        "icomb" => crate::icomb::demo(),
        "sca" | "evolve" => rest
            .split_whitespace()
            .map(|w| match sca::evolve(w) {
                Ok(o) => format!("{} → {}", w, o),
                Err(e) => format!("{} ! {}", w, e),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        // the general SCArs engine: apply arbitrary rules `FROM>TO/PRE_POST` to one word
        "scar" => {
            let mut parts = rest.split_whitespace();
            match parts.next() {
                None => "usage: scar <word> <rule>..  e.g.  scar kasa k>g s>z/a_a".into(),
                Some(word) => {
                    let rules: Vec<String> = parts.map(|s| s.to_string()).collect();
                    match sca::run_sca(word, &rules) {
                        Ok(out) => {
                            let rs = if rules.is_empty() { "(no rules)".to_string() } else { rules.join("  ") };
                            format!("{}  --[ {} ]-->  {}", word, rs, out)
                        }
                        Err(e) => format!("sca error: {}", e),
                    }
                }
            }
        }
        // the economic planner: `plan [iters [demand_steel demand_grain]]` (thousandths)
        "plan" => {
            let nums: Vec<u128> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            let iters = nums.first().copied().unwrap_or(60) as u64;
            let ds = nums.get(1).copied().unwrap_or(0);
            let dg = nums.get(2).copied().unwrap_or(1000);
            crate::plan::plan_report(ds, dg, iters)
        }
        "libs" => {
            let mut out = String::from("loaded modules:\n");
            out.push_str("  built-in: ");
            out.push_str(&latte::builtin_lib_names().join(" "));
            let rt = latte::runtime_lib_names();
            if !rt.is_empty() {
                out.push_str("\n  compiled: ");
                out.push_str(&rt.join(" "));
            }
            out
        }
        "help" => run_tool(""),
        // Oberon-style `Module.command args`: call arm `command` of a loaded module on the
        // parsed argument(s). Because every loaded library is linked into one namespace, the
        // arm is reachable by name — so the system's command set is just Latte source.
        _ if head.contains('.') && head.split('.').all(|p| is_ident(p)) => {
            let arm = head.rsplit('.').next().unwrap_or(head);
            let expr = if rest.is_empty() {
                format!("({} 0)", arm)
            } else {
                format!("({} {})", arm, rest)
            };
            eval_expr(&expr)
        }
        // A bare parenthesised expression is also accepted as an expression to evaluate.
        _ if head.starts_with('(') => eval_expr(cmd),
        _ => format!(
            "unknown command '{}'. try: eval | type | sca | icomb | libs, or Module.command args",
            head
        ),
    }
}

/// True for a Latte identifier / module name component.
fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Evaluate a Latte expression with the whole library scope, via Anvil (native) with an
/// interpreter fallback, and render the resulting noun for the console.
fn eval_expr(expr: &str) -> String {
    let libs: Vec<String> = crate::latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    if let Some(n) = crate::rustgen::run_native_noun(expr, &refs) {
        return render_noun(&n);
    }
    match latte::run_with_libs(expr, &refs) {
        Ok(v) => render_noun(&v),
        Err(e) => format!("error: {}", e),
    }
}

/// Render a noun for display: small atoms as decimals, multi-byte printable atoms as text
/// (cords), and cells as `[a b]`. (Every atom is both a number and a possible string; in an
/// eval console the number is the expected reading, e.g. `(mul 6 7)` is `42`, not `"*"`.)
fn render_noun(n: &N) -> String {
    match &**n {
        Knot::Atom(a) => {
            if a.bytes_le().len() >= 2 {
                if let Some(s) = a.as_cord() {
                    return s;
                }
            }
            a.to_u128()
                .map(|x| x.to_string())
                .unwrap_or_else(|| format!("{:?}", a))
        }
        Knot::Cell(h, t) => format!("[{} {}]", render_noun(h), render_noun(t)),
    }
}

/// Parse `key=value` (URL-decoded) out of a raw query string.
fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(percent_decode(v));
            }
        }
    }
    None
}

/// Resolve a Facet page filename to a path inside `root`. Rejects anything that is not a
/// bare `*.facet` filename (no slashes, no `..`), so editing can't escape the site root.
fn safe_facet_path(root: &str, name: Option<&str>) -> Option<std::path::PathBuf> {
    let name = name?;
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    if !name.ends_with(".facet") {
        return None;
    }
    Some(std::path::Path::new(root).join(name))
}

/// The library directory that holds the system's `*.lat` source. The GUI root is the site
/// directory (`lib/site`); the libraries sit one level up (`lib`).
fn lib_dir(root: &str) -> Option<std::path::PathBuf> {
    std::path::Path::new(root).parent().map(|p| p.to_path_buf())
}

/// The documentation directory (`docs/`), a sibling of the library directory.
fn docs_dir(root: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(root)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("docs"));
    match p {
        Some(d) if d.as_os_str().is_empty() == false && d.exists() => d,
        _ => std::path::PathBuf::from("docs"),
    }
}

/// A safe documentation name (identifier with hyphens; no separators or `..`).
fn safe_doc_name(name: Option<&str>) -> Option<String> {
    let name = name?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    Some(name.to_string())
}

/// A safe bare module name (identifier only — no path separators, no extension).
fn safe_lib_name(name: Option<&str>) -> Option<String> {
    let name = name?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(name.to_string())
}

/// Find the `core NAME` declared in a module source (for naming the on-disk file).
fn find_core_name(src: &str) -> Option<String> {
    let mut toks = src.split_whitespace();
    while let Some(t) = toks.next() {
        if t == "core" {
            return toks.next().map(|s| s.to_string());
        }
    }
    None
}

// ----- the HTTP semantics (pure; unit-tested) -------------------------------
fn respond_for(req: &Request, res: Option<Resource>) -> Response {
    if req.method == "BAD" {
        return simple(400, "text/plain; charset=utf-8", b"400 Bad Request".to_vec());
    }
    if req.method != "GET" && req.method != "HEAD" {
        let mut r = simple(405, "text/plain; charset=utf-8", b"405 Method Not Allowed".to_vec());
        r.headers.push(("Allow".into(), "GET, HEAD".into()));
        return r;
    }
    let res = match res {
        Some(r) => r,
        None => {
            return simple(
                404,
                "text/html; charset=utf-8",
                format!("<!doctype html><meta charset=utf-8><h1>404</h1><p>No page for <code>{}</code></p>", html_escape(&req.path)).into_bytes(),
            )
        }
    };

    let etag = etag_of(&res.body);
    let lm = res.last_modified.map(httpdate);
    let cache = if res.cacheable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    // conditional GET — revalidation by ETag or modification time
    let inm_match = req.header("if-none-match").map(|v| v.split(',').any(|t| t.trim() == etag)).unwrap_or(false);
    let ims_match = match (req.header("if-modified-since"), &lm) {
        (Some(ims), Some(lm)) => ims == lm,
        _ => false,
    };
    if inm_match || ims_match {
        let mut headers = base_headers(&res.ctype, cache);
        headers.push(("ETag".into(), etag));
        if let Some(lm) = lm {
            headers.push(("Last-Modified".into(), lm));
        }
        return Response { status: 304, headers, body: Vec::new(), is_body_suppressed: true };
    }

    // range request — partial content
    if let Some(rh) = req.header("range") {
        let len = res.body.len() as u64;
        match parse_range(rh, len) {
            Some((start, end)) => {
                let slice = res.body[start as usize..=end as usize].to_vec();
                let mut headers = base_headers(&res.ctype, cache);
                headers.push(("ETag".into(), etag));
                if let Some(lm) = lm {
                    headers.push(("Last-Modified".into(), lm));
                }
                headers.push(("Accept-Ranges".into(), "bytes".into()));
                headers.push(("Content-Range".into(), format!("bytes {}-{}/{}", start, end, len)));
                headers.push(("Content-Length".into(), slice.len().to_string()));
                return Response { status: 206, headers, body: slice, is_body_suppressed: false };
            }
            None if rh.trim_start().starts_with("bytes=") => {
                let mut headers = base_headers(&res.ctype, cache);
                headers.push(("Content-Range".into(), format!("bytes */{}", len)));
                headers.push(("Content-Length".into(), "0".into()));
                return Response { status: 416, headers, body: Vec::new(), is_body_suppressed: false };
            }
            None => {} // unrecognized Range unit: ignore, serve 200
        }
    }

    // plain 200
    let mut headers = base_headers(&res.ctype, cache);
    headers.push(("ETag".into(), etag));
    if let Some(lm) = lm {
        headers.push(("Last-Modified".into(), lm));
    }
    headers.push(("Accept-Ranges".into(), "bytes".into()));
    headers.push(("Content-Length".into(), res.body.len().to_string()));
    Response { status: 200, headers, body: res.body, is_body_suppressed: false }
}

fn base_headers(ctype: &str, cache: &str) -> Vec<(String, String)> {
    vec![
        ("Date".into(), httpdate(now_secs())),
        ("Server".into(), "Hymn".into()),
        ("Content-Type".into(), ctype.to_string()),
        ("Cache-Control".into(), cache.to_string()),
    ]
}

fn simple(status: u16, ctype: &str, body: Vec<u8>) -> Response {
    let mut headers = base_headers(ctype, "no-cache");
    headers.push(("Content-Length".into(), body.len().to_string()));
    Response { status, headers, body, is_body_suppressed: false }
}

fn etag_of(body: &[u8]) -> String {
    let d = sha3::sha3_256(body);
    format!("\"{}\"", &sha3::hex(&d)[..16])
}

fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
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

// ----- writing --------------------------------------------------------------
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
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn log_line(peer: &str, req: &Request, resp: &Response) {
    println!("Hymn {} \"{} {} {}\" {} {}B", peer, req.method, req.path, req.version, resp.status, resp.body.len());
}

// ----- small helpers --------------------------------------------------------
fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn percent_decode(s: &str) -> String {
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

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Format unix seconds as an RFC 1123 HTTP date (always GMT).
fn httpdate(secs: u64) -> String {
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

    fn req(method: &str, headers: &[(&str, &str)]) -> Request {
        let mut h = HashMap::new();
        for (k, v) in headers {
            h.insert(k.to_ascii_lowercase(), v.to_string());
        }
        Request { method: method.into(), path: "/x".into(), version: "HTTP/1.1".into(), headers: h, keep_alive: true, body: Vec::new(), query: String::new() }
    }
    fn resource() -> Resource {
        Resource { body: b"hello world".to_vec(), ctype: "text/plain".into(), cacheable: true, last_modified: Some(1_000_000) }
    }
    fn header<'a>(r: &'a Response, k: &str) -> Option<&'a str> {
        r.headers.iter().find(|(hk, _)| hk.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }

    #[test]
    fn plain_get_has_validators() {
        let r = respond_for(&req("GET", &[]), Some(resource()));
        assert_eq!(r.status, 200);
        assert!(header(&r, "ETag").is_some());
        assert_eq!(header(&r, "Accept-Ranges"), Some("bytes"));
        assert_eq!(header(&r, "Content-Length"), Some("11"));
        assert!(header(&r, "Date").unwrap().ends_with("GMT"));
    }

    #[test]
    fn conditional_etag_304() {
        let etag = etag_of(b"hello world");
        let r = respond_for(&req("GET", &[("If-None-Match", &etag)]), Some(resource()));
        assert_eq!(r.status, 304);
        assert!(r.is_body_suppressed);
        assert_eq!(header(&r, "ETag"), Some(etag.as_str()));
    }

    #[test]
    fn conditional_modified_since_304() {
        let lm = httpdate(1_000_000);
        let r = respond_for(&req("GET", &[("If-Modified-Since", &lm)]), Some(resource()));
        assert_eq!(r.status, 304);
    }

    #[test]
    fn range_206() {
        let r = respond_for(&req("GET", &[("Range", "bytes=0-4")]), Some(resource()));
        assert_eq!(r.status, 206);
        assert_eq!(r.body, b"hello");
        assert_eq!(header(&r, "Content-Range"), Some("bytes 0-4/11"));
        assert_eq!(header(&r, "Content-Length"), Some("5"));
    }

    #[test]
    fn range_suffix_and_open() {
        assert_eq!(parse_range("bytes=-3", 11), Some((8, 10)));
        assert_eq!(parse_range("bytes=6-", 11), Some((6, 10)));
        assert_eq!(parse_range("bytes=0-100", 11), Some((0, 10))); // clamped
    }

    #[test]
    fn range_unsatisfiable_416() {
        let r = respond_for(&req("GET", &[("Range", "bytes=99-100")]), Some(resource()));
        assert_eq!(r.status, 416);
        assert_eq!(header(&r, "Content-Range"), Some("bytes */11"));
    }

    #[test]
    fn method_not_allowed_405() {
        let r = respond_for(&req("POST", &[]), Some(resource()));
        assert_eq!(r.status, 405);
        assert_eq!(header(&r, "Allow"), Some("GET, HEAD"));
    }

    #[test]
    fn missing_is_404() {
        let r = respond_for(&req("GET", &[]), None);
        assert_eq!(r.status, 404);
    }


    #[test]
    fn run_tool_is_thread_safe() {
        // the GUI serves requests on a thread per connection; run_tool -> run_with_libs
        // registers jets and reduces on Loom. Hammer it from many threads at once.
        use std::thread;
        let handles: Vec<_> = (0..24)
            .map(|_| {
                thread::spawn(|| {
                    (run_tool("eval (mul 6 7)"), run_tool("sca ligā"), run_tool("type head [1 2]"))
                })
            })
            .collect();
        for h in handles {
            let (a, b, c) = h.join().unwrap();
            assert_eq!(a, "42");
            assert_eq!(b, "ligā → liɣō");
            assert_eq!(c, "head [1 2] : @");
        }
    }

    #[test]
    fn run_tool_dispatches() {
        assert_eq!(run_tool("eval (add 2 3)"), "5");
        assert_eq!(run_tool("type head [1 2]"), "head [1 2] : @");
        assert_eq!(run_tool("sca ligā"), "ligā → liɣō");
        // eval reaches the numeric libraries
        assert_eq!(run_tool("eval (tsum (tfromnats [1 [2 [3 [4 0]]]]))"), "[0 10000]");
    }

    #[test]
    fn api_render_evaluates_facet() {
        let mut h = std::collections::HashMap::new();
        let req = Request {
            method: "POST".into(),
            path: "/api/render".into(),
            version: "HTTP/1.1".into(),
            headers: { h.insert("content-type".into(), "text/plain".into()); h },
            keep_alive: true,
            body: "<b>{{ SCArs.evolve(\"ligā\") }}</b>".as_bytes().to_vec(),
            query: String::new(),
        };
        let r = api_handle(&req, &None, &None, ".");
        assert_eq!(r.status, 200);
        assert_eq!(String::from_utf8_lossy(&r.body), "<b>liɣō</b>");
    }

    #[test]
    fn httpdate_known_epoch() {
        // 0 = Thu, 01 Jan 1970 00:00:00 GMT
        assert_eq!(httpdate(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        // 784111777 = Sun, 06 Nov 1994 08:49:37 GMT (the RFC example)
        assert_eq!(httpdate(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
    }

    #[test]
    fn module_command_dispatch() {
        // `Module.cmd args` calls a Latte arm of a loaded module (the Oberon command model).
        assert_eq!(run_tool("Tool.fib 10"), "55");
        assert_eq!(run_tool("Tool.fact 5"), "120");
        assert_eq!(run_tool("Tool.gcd [1071 462]"), "21");
        assert_eq!(run_tool("Tool.greet"), "orpheus"); // a command that returns a cord
        // a bare parenthesised expression evaluates too
        assert_eq!(run_tool("(mul 6 7)"), "42");
    }

    #[test]
    fn libs_lists_loaded_modules() {
        let out = run_tool("libs");
        assert!(out.contains("tool"));
        assert!(out.contains("std"));
    }

    #[test]
    fn tool_verbs_scar_and_plan() {
        // the general SCArs engine: intervocalic voicing turns kasa into gaza
        let out = run_tool("scar kasa k>g s>z/a_a");
        assert!(out.contains("kasa"));
        assert!(out.contains("gaza"));
        // the planner produces its two-sector report
        assert!(run_tool("plan 10 0 1000").to_lowercase().contains("economy"));
        // the Ligurian evolve path is unchanged
        assert_eq!(run_tool("sca ligā"), "ligā → liɣō");
    }

    #[test]
    fn oberon_edit_compile_run_loop() {
        // Compile a module into the running system, then call its arm by `Module.cmd`.
        let src = "import std\ncore demo_loop\n  dl_op = fn [n] -> (add n n)\nend";
        crate::latte::compile_and_register(src).unwrap();
        assert_eq!(run_tool("demo_loop.dl_op 21"), "42");
        // Recompile with a changed body — the new definition is live immediately.
        let src2 = "import std\ncore demo_loop\n  dl_op = fn [n] -> (mul n 10)\nend";
        crate::latte::compile_and_register(src2).unwrap();
        assert_eq!(run_tool("demo_loop.dl_op 4"), "40");
    }

    #[test]
    fn lib_name_and_dir_helpers() {
        assert_eq!(safe_lib_name(Some("std")), Some("std".to_string()));
        assert_eq!(safe_lib_name(Some("my_mod9")), Some("my_mod9".to_string()));
        assert_eq!(safe_lib_name(Some("../etc/passwd")), None);
        assert_eq!(safe_lib_name(Some("a/b")), None);
        assert_eq!(safe_lib_name(Some("")), None);
        assert_eq!(find_core_name("import std\ncore widget\n end"), Some("widget".to_string()));
        // the library dir is the parent of the (site) root
        assert_eq!(lib_dir("lib/site"), Some(std::path::PathBuf::from("lib")));
    }
}
