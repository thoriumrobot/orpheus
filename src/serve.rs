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
use crate::{check, facet, latte, sca};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;
use crate::httpd::{self, Request, Response, simple, base_headers, etag_of, parse_range, content_type, percent_decode, httpdate};

// ----- the GUI/web routing surface over the shared httpd core ---------------
struct HymnHandler {
    root: String,
    editor: Option<EditorHandle>,
    chess: Option<ChessHandle>,
}

impl httpd::Handler for HymnHandler {
    fn handle(&self, req: &Request) -> Response {
        if req.path.starts_with("/api/") {
            api_handle(req, &self.editor, &self.chess, &self.root)
        } else if self.editor.is_some() && req.path == "/" {
            // the GUI's home is the System console
            respond_for(req, resolve(&self.root, "/system", ""))
        } else {
            respond_for(req, resolve(&self.root, &req.path, &req.query))
        }
    }
}

fn serve_with(listen: &str, root: &str, editor: Option<EditorHandle>, chess: Option<ChessHandle>) {
    println!("Hymn — Orpheus web server (HTTP/1.1)");
    println!("  hosting '{}' at http://{}/", root, listen);
    if editor.is_some() {
        println!("  WYSIWYG editor at  http://{}/editor   (live Facet preview, save/load)", listen);
    }
    println!("  keep-alive · ETag/304 · Range/206 · fonts · SCArs-powered Facet pages");
    let handler = HymnHandler { root: root.to_string(), editor, chess };
    if let Err(e) = httpd::serve(listen, std::sync::Arc::new(handler)) {
        eprintln!("Hymn: cannot bind {}: {}", listen, e);
    }
}


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

// ----- request / response model ---------------------------------------------
struct Resource {
    body: Vec<u8>,
    ctype: String,
    cacheable: bool,
    last_modified: Option<u64>, // unix seconds
}

// ----- connection loop ------------------------------------------------------
/// Read one request (request line + headers). Returns None at clean EOF.
// ----- routing --------------------------------------------------------------
fn resolve(root: &str, path: &str, query: &str) -> Option<Resource> {
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
        // user input: every query parameter is seeded into the page so In.field(name, default) can
        // read it — this is what makes a Facet page an interactive interface.
        let params: Vec<(String, String)> = query
            .split('&')
            .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_string(), percent_decode(v))))
            .collect();
        let body = match facet::render_with(&src, &params) {
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
        // live widget evaluation: POST `expr=<facet-expr>&name=value&…`; render the single
        // expression against the supplied inputs and return its text. This is what powers
        // Live.box — the page's interactive widgets call back here as the user types. Results are
        // a pure function of the request body, so a small bounded cache makes repeats instant.
        ("POST", "/api/eval") => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            let mut expr = String::new();
            let mut inputs: Vec<(String, String)> = Vec::new();
            for pair in body.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    let (k, v) = (percent_decode(k), percent_decode(v));
                    if k == "expr" {
                        expr = v;
                    } else {
                        inputs.push((k, v));
                    }
                }
            }
            // eval_live memoizes on the canonical (expr, inputs) keyed by library generation, so it
            // is both fast and correctly invalidated when a library is edited — unlike a raw-body
            // cache, which would serve stale results after a lib change.
            let out = facet::eval_live(&expr, &inputs);
            simple(200, "text/plain; charset=utf-8", out.into_bytes())
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
                        .filter(|n| n.ends_with(".facet") || n.ends_with(".md"))
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
        // Look up the definition of a function across all modules, by running the
        // `lookup` library (lk_lookup) — the Latte tool — over each module's source.
        // GET /api/defn?name=bt_search  ->  "module btree\n\n  <comment+definition lines>"
        ("GET", "/api/defn") => {
            match query_param(&req.query, "name").map(|n| n.trim().to_string()) {
                Some(name) if is_ident(&name) => {
                    simple(200, "text/plain; charset=utf-8", lookup_definition(&name, root).into_bytes())
                }
                _ => simple(400, "text/plain; charset=utf-8",
                    b"highlight a function name first (letters, digits, '_', '.')".to_vec()),
            }
        }
        // GET /api/symbols?name=dot  ->  the database-backed symbol report: which
        // loaded modules define the name, their arities, and a shadowing warning.
        // The host hands every library's surface to lib/symbols.lat (built on the
        // composed database) and renders the [%html] result it returns.
        ("GET", "/api/symbols") => {
            match query_param(&req.query, "name").map(|n| n.trim().to_string()) {
                Some(name) if is_ident(&name) => {
                    simple(200, "text/html; charset=utf-8", symbol_index_html(&name).into_bytes())
                }
                _ => simple(400, "text/plain; charset=utf-8",
                    b"name a symbol (letters, digits, '_')".to_vec()),
            }
        }
        // GET /api/findb?market=btc&n=24  ->  a database-backed price dashboard:
        // load the last n closes of the market into lib/findb.lat's store (built on
        // the composed database), read them back for an SVG sparkline + summary
        // statistics + a lag-1 regression model, and return the [%html] it renders.
        ("GET", "/api/findb") => {
            let market = query_param(&req.query, "market")
                .map(|m| m.trim().to_lowercase())
                .unwrap_or_else(|| "btc".to_string());
            let n = query_param(&req.query, "n")
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(24);
            if !market.is_empty() && market.chars().all(|c| c.is_ascii_alphanumeric()) {
                simple(200, "text/html; charset=utf-8", findb_dash_html(&market, n).into_bytes())
            } else {
                simple(400, "text/plain; charset=utf-8",
                    b"market must be alphanumeric (e.g. btc, eth)".to_vec())
            }
        }
        // The PERSISTENT database (src/dbservice.rs): named databases backed by an
        // on-disk write-ahead log, surviving restarts. op = dash|get|query|history|list
        // (GET) or put|delete (POST, record expression in the body).
        ("GET", "/api/db") => {
            let svc = crate::dbservice::service();
            let mut s = svc.lock().unwrap();
            let name = query_param(&req.query, "name").unwrap_or_default();
            let key = query_param(&req.query, "key").unwrap_or_default();
            let field = query_param(&req.query, "field").unwrap_or_default();
            let op = query_param(&req.query, "op").unwrap_or_else(|| "dash".into());
            if op != "list" && !is_db_name(&name) {
                return simple(400, "text/plain; charset=utf-8", b"name must be alphanumeric".to_vec());
            }
            let body = match op.as_str() {
                "list" => Ok(s.list().into_iter()
                    .map(|(n, k, w)| format!("{}  ({} keys, {} log entries)", n, k, w))
                    .collect::<Vec<_>>().join("\n")),
                "get" => s.get(&name, &key),
                "keys" => s.keys(&name).map(|ks| ks.join("\n")),   // the sync protocol's diff basis
                "rec" => s.rec(&name, &key),                         // re-evaluable record (sync wire format)
                "query" => s.query_html(&name, &field),
                "history" => s.history_html(&name, &key),
                _ => s.dash_html(&name),
            };
            match body {
                Ok(t) => simple(200, "text/plain; charset=utf-8", t.into_bytes()),
                Err(e) => simple(200, "text/plain; charset=utf-8", format!("db error: {}", e).into_bytes()),
            }
        }
        ("POST", "/api/db") => {
            let svc = crate::dbservice::service();
            let mut s = svc.lock().unwrap();
            let name = query_param(&req.query, "name").unwrap_or_default();
            let key = query_param(&req.query, "key").unwrap_or_default();
            let op = query_param(&req.query, "op").unwrap_or_else(|| "put".into());
            if !is_db_name(&name) || key.is_empty() {
                return simple(400, "text/plain; charset=utf-8", b"need an alphanumeric name and a key".to_vec());
            }
            let rec = String::from_utf8_lossy(&req.body).trim().to_string();
            let r = match op.as_str() {
                "delete" => s.delete(&name, &key).map(|_| format!("deleted {} from {}", key, name)),
                _ => s.put(&name, &key, &rec).map(|_| format!("stored {} in {}", key, name)),
            };
            match r {
                Ok(t) => simple(200, "text/plain; charset=utf-8", t.into_bytes()),
                Err(e) => simple(200, "text/plain; charset=utf-8", format!("db error: {}", e).into_bytes()),
            }
        }
        // Compile a module's source into the running system and persist it to disk if possible.
        // This is the GUI's edit -> compile -> run loop applied to the system's own modules.
        // ---- TEXTS: the user's documents-with-objects, saved under <root>/text/ ----
        // A text is markdown whose ```tool <command>``` fences record embedded
        // dynamic objects; the GUI re-runs each command on load to rehydrate them.
        ("GET", "/api/texts") => {
            let mut names: Vec<String> = Vec::new();
            if let Some(dir) = text_dir(root) {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        if let Some(n) = e.file_name().to_str() {
                            if let Some(stem) = n.strip_suffix(".md") {
                                names.push(stem.to_string());
                            }
                        }
                    }
                }
            }
            names.sort();
            simple(200, "text/plain; charset=utf-8", names.join("\n").into_bytes())
        }
        ("GET", "/api/text") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let p = text_dir(root).map(|d| d.join(format!("{}.md", name)));
                match p.and_then(|p| std::fs::read_to_string(p).ok()) {
                    Some(t) => simple(200, "text/plain; charset=utf-8", t.into_bytes()),
                    None => simple(404, "text/plain; charset=utf-8", b"no such text".to_vec()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        ("POST", "/api/text") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let body = String::from_utf8_lossy(&req.body).into_owned();
                match text_dir(root) {
                    Some(dir) => {
                        let _ = std::fs::create_dir_all(&dir);
                        match std::fs::write(dir.join(format!("{}.md", name)), body.as_bytes()) {
                            Ok(_) => simple(200, "text/plain; charset=utf-8", b"saved".to_vec()),
                            Err(e) => simple(500, "text/plain; charset=utf-8", format!("save failed: {}", e).into_bytes()),
                        }
                    }
                    None => simple(500, "text/plain; charset=utf-8", b"no text dir".to_vec()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        ("POST", "/api/debug") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut focus: Option<String> = None;
            let mut expr_lines: Vec<&str> = Vec::new();
            for line in body.lines() {
                if let Some(v) = line.strip_prefix("break=") {
                    focus = Some(v.trim().to_string());
                } else {
                    expr_lines.push(line);
                }
            }
            let expr = expr_lines.join("\n");
            let libs: Vec<String> = crate::latte::all_libs();
            let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
            fn node_json(n: &crate::latte::TraceNode) -> String {
                let kids: Vec<String> = n.children.iter().map(node_json).collect();
                format!(
                    "{{\"name\":\"{}\",\"args\":\"{}\",\"result\":\"{}\",\"children\":[{}]}}",
                    json_escape(&n.name),
                    json_escape(&n.args),
                    json_escape(n.result.as_deref().unwrap_or("<crash>")),
                    kids.join(",")
                )
            }
            let out = match crate::latte::debug_trace(&expr, &refs, focus.as_deref()) {
                Ok((result, roots, truncated)) => {
                    let tree: Vec<String> = roots.iter().map(node_json).collect();
                    format!(
                        "{{\"result\":\"{}\",\"truncated\":{},\"tree\":[{}]}}",
                        json_escape(&crate::net::show_state(&result)),
                        truncated,
                        tree.join(",")
                    )
                }
                Err(e) => format!("{{\"error\":\"{}\"}}", json_escape(&e)),
            };
            simple(200, "application/json; charset=utf-8", out.into_bytes())
        }
        // xiangqi: verbs new / move f t / ai / state -> the JSON game state.
        // The opponent is the TRAINED MODEL (lib/xiangqiml.lat) at 2-ply.
        ("POST", "/api/xiangqi") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut it = body.split_whitespace();
            let verb = it.next().unwrap_or("state");
            let f = it.next().and_then(|s| s.parse::<u128>().ok());
            let t = it.next().and_then(|s| s.parse::<u128>().ok());
            simple(200, "application/json; charset=utf-8", crate::game::xq_command(verb, f, t).into_bytes())
        }
        // ---- THE CONLANG SUITE -------------------------------------------------
        // GET /api/soundlib -> the attested-change library (JSON). POST with
        // `changes=` (ordered ids) and `words=` lines -> the assembled SCArs
        // file + before/after rows: the library tool CALLING the SCArs engine.
        ("GET", "/api/soundlib") => {
            let mut j = String::from("[");
            for (i, (id, name, attested, desc, rules)) in crate::conlang::SOUND_CHANGES.iter().enumerate() {
                if i > 0 {
                    j.push(',');
                }
                j.push_str(&format!(
                    "{{\"id\":\"{}\",\"name\":\"{}\",\"attested\":\"{}\",\"desc\":\"{}\",\"rules\":\"{}\"}}",
                    json_escape(id), json_escape(name), json_escape(attested), json_escape(desc), json_escape(rules)
                ));
            }
            j.push(']');
            simple(200, "application/json; charset=utf-8", j.into_bytes())
        }
        ("POST", "/api/soundlib") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut changes: Vec<String> = Vec::new();
            let mut words: Vec<String> = Vec::new();
            let mut file: Option<String> = None;
            for line in body.lines() {
                if let Some(v) = line.strip_prefix("changes=") {
                    changes = v.split_whitespace().map(String::from).collect();
                } else if let Some(v) = line.strip_prefix("words=") {
                    words = v.split_whitespace().map(String::from).collect();
                } else if let Some(v) = line.strip_prefix("file=") {
                    file = safe_doc_name(Some(v.trim()));
                }
            }
            if words.is_empty() {
                words = ["pater", "bhrater", "lupo", "skola"].iter().map(|s| s.to_string()).collect();
            }
            // file=NAME applies a STORED ruleset (sca/<name>.sca) directly
            if let Some(f) = file {
                let path = crate::conlang::sca_dir().join(format!("{}.sca", f));
                let res = std::fs::read_to_string(&path)
                    .map_err(|_| format!("no stored ruleset '{}'", f))
                    .map(|sca| {
                        let lines: Vec<String> = sca.lines().map(String::from).collect();
                        let rows: Vec<(String, String)> = words
                            .iter()
                            .map(|w| (w.clone(), crate::sca::run_sca(w, &lines).unwrap_or_else(|e| format!("<error: {}>", e))))
                            .collect();
                        (sca, rows)
                    });
                let out = match res {
                    Ok((sca, rows)) => {
                        let mut j = format!("{{\"sca\":\"{}\",\"rows\":[", json_escape(&sca));
                        for (i, (w, e)) in rows.iter().enumerate() {
                            if i > 0 {
                                j.push(',');
                            }
                            j.push_str(&format!("{{\"from\":\"{}\",\"to\":\"{}\"}}", json_escape(w), json_escape(e)));
                        }
                        j.push_str("]}");
                        j
                    }
                    Err(e) => format!("{{\"error\":\"{}\"}}", json_escape(&e)),
                };
                return simple(200, "application/json; charset=utf-8", out.into_bytes());
            }
            let ids: Vec<&str> = changes.iter().map(|s| s.as_str()).collect();
            match crate::conlang::apply_changes(&ids, &words) {
                Ok((sca, rows)) => {
                    let mut j = format!("{{\"sca\":\"{}\",\"rows\":[", json_escape(&sca));
                    for (i, (w, e)) in rows.iter().enumerate() {
                        if i > 0 {
                            j.push(',');
                        }
                        j.push_str(&format!("{{\"from\":\"{}\",\"to\":\"{}\"}}", json_escape(w), json_escape(e)));
                    }
                    j.push_str("]}");
                    simple(200, "application/json; charset=utf-8", j.into_bytes())
                }
                Err(e) => simple(200, "application/json; charset=utf-8", format!("{{\"error\":\"{}\"}}", json_escape(&e)).into_bytes()),
            }
        }
        // POST /api/phono: the phonology builder — preset= or consonants=/vowels=/
        // patterns=, n=, seed=, changes= (optional) -> generated words, evolved
        // through the change library through SCArs (the modular chain).
        ("POST", "/api/phono") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut preset = String::new();
            let (mut cons, mut vows, mut pats) = (String::new(), String::new(), String::new());
            let mut n = 14usize;
            let mut seed = 42u64;
            let mut changes: Vec<String> = Vec::new();
            for line in body.lines() {
                let mut it = line.splitn(2, '=');
                match (it.next(), it.next()) {
                    (Some("preset"), Some(v)) => preset = v.trim().to_string(),
                    (Some("consonants"), Some(v)) => cons = v.trim().to_string(),
                    (Some("vowels"), Some(v)) => vows = v.trim().to_string(),
                    (Some("patterns"), Some(v)) => pats = v.trim().to_string(),
                    (Some("n"), Some(v)) => n = v.trim().parse().unwrap_or(n),
                    (Some("seed"), Some(v)) => seed = v.trim().parse().unwrap_or(seed),
                    (Some("changes"), Some(v)) => changes = v.split_whitespace().map(String::from).collect(),
                    _ => {}
                }
            }
            let ph = if !cons.is_empty() && !vows.is_empty() {
                crate::conlang::Phonology {
                    consonants: cons.split_whitespace().map(String::from).collect(),
                    vowels: vows.split_whitespace().map(String::from).collect(),
                    patterns: if pats.is_empty() {
                        vec!["CV".into(), "CVC".into()]
                    } else {
                        pats.split_whitespace().map(String::from).collect()
                    },
                }
            } else {
                let pres = crate::conlang::presets();
                let chosen = pres
                    .into_iter()
                    .find(|(id, _, _)| *id == preset)
                    .map(|(_, _, p)| p)
                    .unwrap_or_else(|| crate::conlang::presets().remove(1).2);
                chosen
            };
            let ids: Vec<&str> = changes.iter().map(|s| s.as_str()).collect();
            let report = crate::conlang::phonology_report(&ph);
            match crate::conlang::phonology_pipeline(&ph, n, seed, &ids) {
                Ok((rows, sca)) => {
                    let rep: Vec<String> = report
                        .iter()
                        .map(|(l, m)| format!("{{\"level\":\"{}\",\"msg\":\"{}\"}}", l, json_escape(m)))
                        .collect();
                    let mut j = format!("{{\"report\":[{}],\"phon\":\"{}\",\"sca\":\"{}\",\"rows\":[",
                        rep.join(","),
                        json_escape(&crate::conlang::format_phon(&ph, "stored by the Orpheus phonology builder")),
                        json_escape(&sca));
                    for (i, (w, e)) in rows.iter().enumerate() {
                        if i > 0 {
                            j.push(',');
                        }
                        j.push_str(&format!("{{\"proto\":\"{}\",\"evolved\":\"{}\"}}", json_escape(w), json_escape(e)));
                    }
                    j.push_str("]}");
                    simple(200, "application/json; charset=utf-8", j.into_bytes())
                }
                Err(e) => simple(200, "application/json; charset=utf-8", format!("{{\"error\":\"{}\"}}", json_escape(&e)).into_bytes()),
            }
        }
        // delete a stored text (the GUI's "Default" button: removing a stored
        // tool text restores the built-in version on next open)
        ("DELETE", "/api/text") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let p = text_dir(root).map(|d| d.join(format!("{}.md", name)));
                match p.map(std::fs::remove_file) {
                    Some(Ok(_)) => simple(200, "text/plain; charset=utf-8", b"deleted".to_vec()),
                    _ => simple(404, "text/plain; charset=utf-8", b"no such stored text".to_vec()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        // ---- CONLANG FILES: sca/<name>.sca and phonology/<name>.phon ----------
        ("GET", "/api/scafiles") => {
            let names = list_ext(&crate::conlang::sca_dir(), "sca");
            simple(200, "text/plain; charset=utf-8", names.join("\n").into_bytes())
        }
        ("GET", "/api/scafile") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => match std::fs::read_to_string(crate::conlang::sca_dir().join(format!("{}.sca", name))) {
                Ok(t) => simple(200, "text/plain; charset=utf-8", t.into_bytes()),
                Err(_) => simple(404, "text/plain; charset=utf-8", b"no such ruleset".to_vec()),
            },
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        ("POST", "/api/scafile") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let body = String::from_utf8_lossy(&req.body).into_owned();
                // validate: it must parse as a SCArs rule file before we store it
                if let Err(e) = crate::sca::parse_sca(&body) {
                    return simple(400, "text/plain; charset=utf-8", format!("not a valid .sca: {}", e).into_bytes());
                }
                let dir = crate::conlang::sca_dir();
                let _ = std::fs::create_dir_all(&dir);
                match std::fs::write(dir.join(format!("{}.sca", name)), body.as_bytes()) {
                    Ok(_) => simple(200, "text/plain; charset=utf-8",
                        format!("saved sca/{}.sca — load it anywhere: latte sca --file sca/{}.sca <words>", name, name).into_bytes()),
                    Err(e) => simple(500, "text/plain; charset=utf-8", format!("save failed: {}", e).into_bytes()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        ("GET", "/api/phonfiles") => {
            let names = list_ext(&crate::conlang::phon_dir(), "phon");
            simple(200, "text/plain; charset=utf-8", names.join("\n").into_bytes())
        }
        ("GET", "/api/phonfile") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => match std::fs::read_to_string(crate::conlang::phon_dir().join(format!("{}.phon", name))) {
                Ok(t) => simple(200, "text/plain; charset=utf-8", t.into_bytes()),
                Err(_) => simple(404, "text/plain; charset=utf-8", b"no such phonology".to_vec()),
            },
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        ("POST", "/api/phonfile") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let body = String::from_utf8_lossy(&req.body).into_owned();
                if let Err(e) = crate::conlang::parse_phon(&body) {
                    return simple(400, "text/plain; charset=utf-8", format!("not a valid .phon: {}", e).into_bytes());
                }
                let dir = crate::conlang::phon_dir();
                let _ = std::fs::create_dir_all(&dir);
                match std::fs::write(dir.join(format!("{}.phon", name)), body.as_bytes()) {
                    Ok(_) => simple(200, "text/plain; charset=utf-8", format!("saved phonology/{}.phon", name).into_bytes()),
                    Err(e) => simple(500, "text/plain; charset=utf-8", format!("save failed: {}", e).into_bytes()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        // ---- DRAWINGS: the graphics tool's documents (drawings/<name>.svg) ----
        ("GET", "/api/drawings") => {
            let mut names: Vec<String> = Vec::new();
            if let Some(dir) = drawings_dir(root) {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        if let Some(n) = e.file_name().to_str() {
                            if let Some(stem) = n.strip_suffix(".svg") {
                                names.push(stem.to_string());
                            }
                        }
                    }
                }
            }
            names.sort();
            simple(200, "text/plain; charset=utf-8", names.join("\n").into_bytes())
        }
        ("GET", "/api/drawing") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let p = drawings_dir(root).map(|d| d.join(format!("{}.svg", name)));
                match p.and_then(|p| std::fs::read_to_string(p).ok()) {
                    Some(t) => simple(200, "image/svg+xml; charset=utf-8", t.into_bytes()),
                    None => simple(404, "text/plain; charset=utf-8", b"no such drawing".to_vec()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        ("POST", "/api/drawing") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let body = String::from_utf8_lossy(&req.body).into_owned();
                // only accept what looks like an SVG document
                if !body.trim_start().starts_with("<svg") {
                    return simple(400, "text/plain; charset=utf-8", b"not an svg".to_vec());
                }
                match drawings_dir(root) {
                    Some(dir) => {
                        let _ = std::fs::create_dir_all(&dir);
                        match std::fs::write(dir.join(format!("{}.svg", name)), body.as_bytes()) {
                            Ok(_) => simple(200, "text/plain; charset=utf-8", b"saved".to_vec()),
                            Err(e) => simple(500, "text/plain; charset=utf-8", format!("save failed: {}", e).into_bytes()),
                        }
                    }
                    None => simple(500, "text/plain; charset=utf-8", b"no drawings dir".to_vec()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad name".to_vec()),
        },
        // quit the system: respond, then exit the process (Oberon's System.Quit)
        ("POST", "/api/quit") => {
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(300));
                eprintln!("System.Quit — Orpheus stopped.");
                std::process::exit(0);
            });
            simple(
                200,
                "text/html; charset=utf-8",
                "<!doctype html><html><body style='font-family:ui-monospace,Menlo,monospace;background:#d6d2c4;color:#22222e;display:flex;align-items:center;justify-content:center;height:100vh'><div style='text-align:center'><h2 style='margin:0 0 8px'>Orpheus stopped.</h2><p style='color:#6b6858'>System.Quit — restart with <code>latte serve</code>.</p></div></body></html>".as_bytes().to_vec(),
            )
        }
        // format a Latte source (conservative, compile-checked); body in, body out
        ("POST", "/api/fmt") => {
            let body = String::from_utf8_lossy(&req.body);
            simple(200, "text/plain; charset=utf-8", crate::latte::format_source(&body).into_bytes())
        }
        // ---- Forge: the team-coding log (a persistent local Mocha node; link
        // machines with `latte team --listen/--peer`). Commands, one per request:
        //   share <author> <name>\n<code…>   add a snippet (code = rest of body)
        //   list | names | count | last      views over the shared log
        //   get <name> | by <author> | del <name> | clear
        ("POST", "/api/forge") => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            simple(200, "text/plain; charset=utf-8", forge_command(&body).into_bytes())
        }
        ("POST", "/api/source") => {
            let body = String::from_utf8_lossy(&req.body).into_owned();
            match crate::latte::compile_and_register(&body) {
                Ok(mut msg) => {
                    // persist: SYSTEM libraries (names shipped in lib/) write back to
                    // lib/<name>.lat; everything else is a USER PACKAGE -> pkg/<name>.lat
                    if let Some(name) = find_core_name(&body) {
                        let is_system = crate::latte::builtin_lib_names().contains(&name)
                            || lib_dir(root).map(|d| d.join(format!("{}.lat", name)).exists()).unwrap_or(false);
                        if is_system {
                            if let Some(p) = lib_dir(root).map(|d| d.join(format!("{}.lat", name))) {
                                match std::fs::write(&p, body.as_bytes()) {
                                    Ok(_) => msg.push_str(" — saved to lib/ (a system library)"),
                                    Err(_) => msg.push_str(" — (in-memory only; library dir not writable)"),
                                }
                            }
                        } else {
                            match crate::latte::store_package(&body) {
                                Ok(p) => msg.push_str(&format!(" — stored as package {}", p.display())),
                                Err(_) => msg.push_str(" — (in-memory only; pkg dir not writable)"),
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
                "latte-tutorial" => 1,
                "using-latte-from-the-gui" => 1,
                "facet-language" => 2,
                "scars-sound-changes" => 2,
                "interaction-nets" => 3,
                "adding-libraries" => 4,
                "data-intensive" => 5,
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
        // save an edited document back to docs/<name>.md (Oberon-style: documents are editable
        // in place from within the GUI). The name is validated; writes are confined to docs/.
        ("POST", "/api/doc") => match safe_doc_name(query_param(&req.query, "name").as_deref()) {
            Some(name) => {
                let dir = docs_dir(root);
                let _ = std::fs::create_dir_all(&dir);
                let p = dir.join(format!("{}.md", name));
                match std::fs::write(&p, &req.body) {
                    Ok(_) => simple(200, "text/plain; charset=utf-8", b"saved".to_vec()),
                    Err(e) => simple(500, "text/plain; charset=utf-8", format!("write failed: {}", e).into_bytes()),
                }
            }
            None => simple(400, "text/plain; charset=utf-8", b"bad doc name".to_vec()),
        },
        // economic planning: body = "iters demand_steel demand_grain" (demand in thousandths)
        ("POST", "/api/plan") => {
            let body = String::from_utf8_lossy(&req.body);
            // a body containing `sector` lines is a CUSTOM ECONOMY spec (the full
            // TNS pipeline: values, gross outputs, market steering, harmony);
            // `demo3` runs the built-in 3-sector demo; otherwise the classic
            // two-sector form "iters demand_steel demand_grain".
            if body.contains("sector ") || body.trim() == "demo3" {
                let spec = if body.trim() == "demo3" { crate::plan::demo_spec().to_string() } else { body.into_owned() };
                let out = crate::plan::parse_economy(&spec)
                    .and_then(|eco| crate::plan::plan_report_custom(&eco, 60))
                    .unwrap_or_else(|e| format!("plan: {}", e));
                return simple(200, "text/plain; charset=utf-8", out.into_bytes());
            }
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
        // financial ML front end: run the gold pipeline, return an HTML fragment
        // (metrics + an inline equity/volatility-timing SVG).
        ("POST", "/api/fin") => {
            // body tokens: a bare number = iters; `--direction`/`--dir`/`direction=1`
            // selects the direction task; `horizon=N` sets the prediction horizon.
            let body = String::from_utf8_lossy(&req.body);
            let mut iters: u64 = 80;
            let mut horizon: usize = 1;
            let mut vol_task = true;
            for tok in body.split_whitespace() {
                if let Ok(n) = tok.parse::<u64>() {
                    iters = n;
                } else if tok == "--direction" || tok == "--dir" || tok == "direction=1" {
                    vol_task = false;
                } else if let Some(v) = tok.strip_prefix("horizon=") {
                    horizon = v.parse().unwrap_or(horizon).clamp(1, 30);
                } else if let Some(v) = tok.strip_prefix("iters=") {
                    iters = v.parse().unwrap_or(iters);
                }
            }
            let html = match crate::numerics::market_eval_cfg(iters.clamp(10, 400), horizon, vol_task) {
                Ok(res) => {
                    let svg = crate::numerics::market_chart(&res);
                    let task = if res.vol {
                        format!("predict {}-day-ahead volatility regime (high vs low)", horizon)
                    } else {
                        format!("predict {}-day-ahead direction (up vs down)", horizon)
                    };
                    format!(
                        "<table class='m'>\
                         <tr><td>market</td><td>{} &mdash; {} daily closes, {} &rarr; {}</td></tr>\
                         <tr><td>task</td><td>{}</td></tr>\
                         <tr><td>features</td><td>momentum (ROC 1/3/5/10) + price/MA10 + HAR realized vol (5d, 22d) + last return</td></tr>\
                         <tr><td>validation</td><td>walk-forward &mdash; {} train, {} unseen test days</td></tr>\
                         <tr><td>training acc.</td><td>{:.1}% (in-sample)</td></tr>\
                         <tr><td><b>TEST acc.</b></td><td><b>{:.1}%</b> (out-of-sample)</td></tr>\
                         <tr><td>baseline</td><td>{:.1}% (majority)</td></tr>\
                         <tr><td><b>edge</b></td><td><b>{:+.1} pts</b></td></tr></table>\
                         <div class='svgbox'>{}</div>\
                         <p class='note'>Why crypto: the same leak-aware pipeline is near-chance on gold, so it was \
                         moved to the market where momentum and volatility clustering are strongest. Daily \
                         <i>direction</i> is still hard, but the next-day <i>volatility regime</i> is genuinely \
                         predictable here &mdash; a real, honest out-of-sample edge over baseline (still modest; \
                         not a profit guarantee). The same model earns ~+50 pts on a synthetic mean-reverting \
                         series in the test suite, confirming it learns.</p>",
                        crate::marketdata::MARKET_NAME, res.n, res.d0, res.d1, task, res.split, res.nsamples - res.split,
                        res.train, res.test, res.base, res.test - res.base, svg
                    )
                }
                Err(e) => format!("<p class='note'>error: {}</p>", html_escape(&e)),
            };
            simple(200, "text/html; charset=utf-8", html.into_bytes())
        }
        // GPU compute + GFX render front end: device info, matmul benchmark, Mandelbrot SVG.
        ("POST", "/api/gpu") => {
            let dev = crate::gpu::Device::target();
            let dim = 160usize;
            let a: Vec<f64> = (0..dim * dim).map(|i| ((i * 7 + 1) % 13) as f64 - 6.0).collect();
            let b: Vec<f64> = (0..dim * dim).map(|i| ((i * 5 + 3) % 11) as f64 - 5.0).collect();
            let t0 = std::time::Instant::now();
            let cs = crate::gpu::matmul_serial(&a, &b, dim, dim, dim);
            let ts = t0.elapsed().as_secs_f64();
            let t1 = std::time::Instant::now();
            let cp = crate::gpu::matmul(&a, &b, dim, dim, dim, dev.lanes);
            let tp = t1.elapsed().as_secs_f64();
            let ok = cs == cp;
            let (w, h, cell) = (180usize, 130usize, 3usize);
            let field = crate::gpu::mandelbrot(w, h, 90, dev.lanes);
            let svg = crate::gpu::field_to_svg(&field, w, h, cell);
            let html = format!(
                "<table class='m'>\
                 <tr><td>target device</td><td>{} &mdash; {} GB VRAM, {} CUDA cores, {} SMs</td></tr>\
                 <tr><td>GPU detected</td><td>{}</td></tr>\
                 <tr><td>active backend</td><td>{}</td></tr>\
                 <tr><td>ML kernel</td><td>dense matmul {dim}&times;{dim}&times;{dim} (the neural-net core op)</td></tr>\
                 <tr><td>serial</td><td>{:.3} s</td></tr>\
                 <tr><td>parallel</td><td>{:.3} s &nbsp;({:.1}&times; on {} lanes) &nbsp; results match: {}</td></tr>\
                 </table>\
                 <p class='note'>The kernel set (map / zipWith / reduce / saxpy / dot / matmul / shader) and the \
                 buffer model match what a CUDA backend would use, so targeting the RTX 4070 Ti SUPER is a \
                 drop-in backend swap. This sandbox has no CUDA driver and zero external crates, so kernels run \
                 on the multi-core CPU backend (here {} hardware lane(s)).</p>\
                 <h4>GPU + GFX: per-pixel Mandelbrot shader &rarr; gfx raster</h4>\
                 <div class='svgbox'>{}</div>",
                dev.target, dev.vram_gb, dev.cuda_cores, dev.sm_count,
                if dev.gpu_present { "yes" } else { "no" }, dev.backend,
                ts, tp, if tp > 0.0 { ts / tp } else { 0.0 }, dev.lanes, ok, dev.lanes, svg,
                dim = dim
            );
            simple(200, "text/html; charset=utf-8", html.into_bytes())
        }
        // graphics library: render the Latte gfx demo scene to SVG.
        ("POST", "/api/gfx") => {
            // body = a Latte expression evaluating to a gfx scene (defaults to the demo);
            // a bare `Module.arm args` command form is also accepted.
            let body = String::from_utf8_lossy(&req.body);
            let expr0 = body.trim();
            let expr = if expr0.is_empty() {
                "(demo 0)".to_string()
            } else if expr0.starts_with('(') {
                expr0.to_string()
            } else {
                // `Tool.rings 6` -> `(rings 6)`
                let (head, rest) = match expr0.split_once(char::is_whitespace) {
                    Some((h, r)) => (h, r.trim()),
                    None => (expr0, ""),
                };
                let arm = head.rsplit('.').next().unwrap_or(head);
                if rest.is_empty() { format!("({} 0)", arm) } else { format!("({} {})", arm, rest) }
            };
            let libs: Vec<String> = crate::latte::all_libs();
            let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
            let svg = match crate::rustgen::run_adaptive(&expr, &refs) {
                Ok(scene) => crate::gfx::render_scene(&scene, 330, 270),
                Err(e) => format!("<svg xmlns='http://www.w3.org/2000/svg' width='330' height='40'><text x='6' y='22'>{}</text></svg>", html_escape(&e)),
            };
            simple(200, "image/svg+xml; charset=utf-8", svg.into_bytes())
        }
        // a graphical front end described in Latte (lib/ui.lat): body = `Mod.arm [args]`
        // or a parenthesised expression; returns the panel as JSON for the GUI to render.
        ("POST", "/api/ui") => {
            let body = String::from_utf8_lossy(&req.body);
            let expr0 = body.trim();
            let expr = if expr0.starts_with('(') {
                expr0.to_string()
            } else {
                let (head, rest) = match expr0.split_once(char::is_whitespace) {
                    Some((h, r)) => (h, r.trim()),
                    None => (expr0, ""),
                };
                let arm = head.rsplit('.').next().unwrap_or(head);
                if rest.is_empty() { format!("({} 0)", arm) } else { format!("({} {})", arm, rest) }
            };
            let libs: Vec<String> = crate::latte::all_libs();
            let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
            let out = match crate::rustgen::run_adaptive(&expr, &refs) {
                Ok(n) => match panel_json(&n) {
                    Some(j) => j,
                    None => format!("{{\"error\":\"{} did not return a (panel …) value\"}}", json_escape(expr0)),
                },
                Err(e) => format!("{{\"error\":\"{}\"}}", json_escape(&e)),
            };
            simple(200, "application/json; charset=utf-8", out.into_bytes())
        }
        // trading advisor: run the best model + position sizing, return an HTML fragment.
        // the curated market list — ONE source (marketdata::MARKETS) serving the
        // trade page's selector, the tools page dropdown, and the docs
        ("GET", "/api/markets") => {
            let mut items: Vec<String> = vec!["{\"sym\":\"bonds\",\"label\":\"US Treasuries (duration model)\"}".into()];
            for (sym, label) in crate::marketdata::MARKETS {
                items.push(format!("{{\"sym\":\"{}\",\"label\":\"{}\"}}", sym, label));
            }
            simple(200, "application/json", format!("[{}]", items.join(",")).into_bytes())
        }
        ("POST", "/api/trade") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut account = 10_000.0f64;
            let mut kelly = 0.25f64;
            let mut sentiment: Option<f64> = None;
            let mut live = false;
            let mut news_text: Option<String> = None;
            let mut market = String::from("btc");
            let mut in_news = false;
            let mut news_buf = String::new();
            for line in body.lines() {
                if in_news {
                    news_buf.push_str(line);
                    news_buf.push('\n');
                    continue;
                }
                let mut it = line.splitn(2, '=');
                match (it.next(), it.next()) {
                    (Some("account"), Some(v)) => account = v.trim().parse().unwrap_or(account),
                    (Some("kelly"), Some(v)) => kelly = v.trim().parse().unwrap_or(kelly),
                    (Some("sentiment"), Some(v)) => sentiment = v.trim().parse().ok(),
                    (Some("live"), Some(v)) => live = v.trim() == "1" || v.trim() == "true",
                    (Some("market"), Some(v)) => market = v.trim().to_lowercase(),
                    (Some("news"), _) => in_news = true, // the rest of the body is headlines
                    _ => {}
                }
            }
            if !news_buf.trim().is_empty() {
                news_text = Some(news_buf);
            }
            // The bond market has no daily tape — route its aliases to the bond desk:
            // the finbond model's HTML dashboard (fb_dash, trained through the db->tensor
            // bridge) plus the bond-scored news lean, the same fusion the CLI advisor
            // (`latte trade --market bonds`) sizes positions from.
            if matches!(market.as_str(),
                "bond" | "bonds" | "treasury" | "treasuries" | "ust" | "10y" | "tnote" | "tlt" | "rates" | "duration")
            {
                let libs: Vec<String> = crate::latte::all_libs();
                let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
                // native first (the 4000-iteration training compiles), else the
                // interpreter with a raised fuel budget — the same policy the CLI
                // bond advisor uses (numerics::eval_native_or_interp)
                let dash = crate::numerics::eval_native_or_interp("(fb_dash0 0)", &refs)
                    .ok()
                    .and_then(|n| extract_html(&n))
                    .unwrap_or_else(|| "<p>bond model unavailable</p>".into());
                let lean = news_text
                    .as_deref()
                    .map(|t| crate::sentiment::score_document_bond(t).0)
                    .or_else(|| crate::numerics::docs_stream_bond().map(|(_, agg)| agg));
                let news_html = match (sentiment, lean) {
                    (Some(s), _) => format!("<p><b>News lean (overridden):</b> {:+.2}</p>", s),
                    (None, Some(l)) => format!(
                        "<p><b>News lean, scored for bonds</b> (hawk/dove axis; risk-off = Treasury bid): {:+.2}</p>", l),
                    (None, None) => "<p>No news scored — drop reports in <code>news/</code> or paste headlines.</p>".into(),
                };
                let page = format!(
                    "{}{}<p style='color:#8a8676'>For sized advice (fractional Kelly, volatility-targeted): <code>latte trade --market bonds</code></p>",
                    dash, news_html
                );
                return simple(200, "text/html; charset=utf-8", page.into_bytes());
            }
            let html = match crate::numerics::trade_advice_market(&market, account, kelly, sentiment, live, news_text.as_deref()) {
                Ok(a) => {
                    let mut ta = String::from("<table class='m'><tr><td><b>indicator</b></td><td><b>value</b></td><td><b>vote</b></td></tr>");
                    for r in &a.ta_rows {
                        let (label, val) = match r.name.as_str() {
                            "trend" => ("price vs SMA50", format!("{:+.2}%", r.value)),
                            "mom" => ("10-day momentum", format!("{:+.2}%", r.value)),
                            "rsi" => ("RSI(14)", format!("{:.1}", r.value)),
                            "macd" => ("MACD(12,26,9) hist", format!("{:+.3}%", r.value)),
                            "boll" => ("Bollinger %B(20,2)", format!("{:.0}", r.value)),
                            o => (o, format!("{:.3}", r.value)),
                        };
                        let v = match r.vote { 1 => "<span style='color:#1a6e2a'>+1 bullish</span>", -1 => "<span style='color:#7a1a1a'>&minus;1 bearish</span>", _ => "0 neutral" };
                        ta.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td></tr>", label, val, v));
                    }
                    ta.push_str(&format!("<tr><td><b>composite TA</b></td><td><b>{:+}</b> of &minus;5..+5</td><td><b>signal {:+.2}</b></td></tr></table>", a.ta_score, a.ta_signal));
                    let mut news = String::from("<table class='m'><tr><td><b>date</b></td><td><b>pol</b></td><td><b>headline</b></td></tr>");
                    for r in &a.news {
                        news.push_str(&format!(
                            "<tr><td>{}</td><td>{:+.2}</td><td>{} <span style='color:#777'>&mdash; {}</span></td></tr>",
                            r.date, r.polarity, html_escape(&r.headline), html_escape(&r.source)
                        ));
                    }
                    news.push_str(&format!("<tr><td><b>aggregate</b></td><td><b>{:+.2}</b></td><td>recency-weighted (half-life 3 days){}</td></tr></table>",
                        a.news_sentiment,
                        if a.sentiment != Some(a.news_sentiment) { format!(" &mdash; overridden to {:+.2}", a.sentiment.unwrap_or(0.0)) } else { String::new() }));
                    format!(
                        "<h3 style='margin:4px 0'>Technical analysis (lib/ta.lat, computed on Loom)</h3>{}\
                         <h3 style='margin:10px 0 4px'>News sentiment (Loughran-McDonald, lib/sentiment.lat)</h3>{}\
                         <h3 style='margin:10px 0 4px'>Verdict</h3>\
                         <table class='m'>\
                         <tr><td>market</td><td>{} &nbsp; last ${:.0} &nbsp; <span style='color:#777'>{} ({} .. {})</span></td></tr>\
                         <tr><td>combined lean</td><td><b>{:+.2}</b> = 0.6&middot;TA {:+.2} + 0.4&middot;news {:+.2} &rarr; {}</td></tr>\
                         <tr><td>realized vol/day</td><td>{:.2}% (target {:.2}%) &mdash; predicted {}-vol regime</td></tr>\
                         <tr><td>momentum hit rate</td><td>{:.1}% out-of-sample</td></tr>\
                         <tr><td>volatility model</td><td>{:.1}% test vs {:.1}% baseline ({})</td></tr>\
                         <tr><td>Kelly: binary / &mu;&#47;&sigma;&sup2; / applied</td><td>{:+.3} / {} / {:.3}  (sized on the smaller estimate)</td></tr>\
                         <tr><td><b>advice</b></td><td><b>{}</b></td></tr>{}\
                         </table>\
                         <p class='note'>Five classical indicators vote in Latte and real, dated headlines are scored in \
                         Latte; the leans fuse 60/40 and the size is risk-controlled by the volatility model (the \
                         dependable edge) with fractional Kelly. <b>Research demo, not financial advice.</b></p>",
                        ta, news,
                        a.market, a.last_price, html_escape(&a.data_note), a.span.0, a.span.1,
                        a.combined, a.ta_signal, a.sentiment.unwrap_or(0.0), a.direction,
                        a.realized_vol, a.target_vol, a.vol_regime,
                        a.dir_hitrate, a.model_acc, a.model_base, if a.model_acc > a.model_base { "a real edge" } else { "NO edge here" }, a.kelly_full,
                        a.kelly_mv.map(|k| format!("{:+.3}", k)).unwrap_or_else(|| "n/a".into()), a.kelly_used, html_escape(&a.action),
                        if a.exposure > 0.0 { format!("<tr><td>notional</td><td>${:.0} of ${:.0}</td></tr>", a.dollars, a.account) } else { String::new() }
                    )
                }
                Err(e) => format!("<p class='note'>error: {}</p>", html_escape(&e)),
            };
            simple(200, "text/html; charset=utf-8", html.into_bytes())
        }
        // standalone technical analysis: body "live=0/1\nwin=N" -> HTML table fragment
        ("POST", "/api/ta") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut live = false;
            let mut win = 120usize;
            let mut market = String::from("btc");
            for line in body.lines() {
                let mut it = line.splitn(2, '=');
                match (it.next(), it.next()) {
                    (Some("live"), Some(v)) => live = v.trim() == "1" || v.trim() == "true",
                    (Some("win"), Some(v)) => win = v.trim().parse().unwrap_or(win),
                    (Some("market"), Some(v)) => market = v.trim().to_lowercase(),
                    _ => {}
                }
            }
            let (closes, span, note) = match crate::marketdata::closes_market(&market, live) {
                Ok(t) => t,
                Err(e) => return simple(200, "text/html; charset=utf-8", format!("<p class='note'>{}</p>", html_escape(&e)).into_bytes()),
            };
            let html = match crate::numerics::ta_votes(&closes, win) {
                Ok((rows, score)) => {
                    let mut t = format!(
                        "<p class='note'>{} &mdash; last ${:.0} &mdash; {} ({} .. {}), window {} days</p>\
                         <table class='m'><tr><td><b>indicator</b></td><td><b>value</b></td><td><b>vote</b></td></tr>",
                        crate::marketdata::market_label(&market),
                        *closes.last().unwrap_or(&0) as f64 / 100.0,
                        html_escape(&note), span.0, span.1, win
                    );
                    for r in &rows {
                        let (label, val) = match r.name.as_str() {
                            "trend" => ("price vs SMA50", format!("{:+.2}%", r.value)),
                            "mom" => ("10-day momentum", format!("{:+.2}%", r.value)),
                            "rsi" => ("RSI(14)", format!("{:.1}", r.value)),
                            "macd" => ("MACD(12,26,9) hist", format!("{:+.3}%", r.value)),
                            "boll" => ("Bollinger %B(20,2)", format!("{:.0}", r.value)),
                            o => (o, format!("{:.3}", r.value)),
                        };
                        let v = match r.vote { 1 => "<span style='color:#1a6e2a'>+1 bullish</span>", -1 => "<span style='color:#7a1a1a'>&minus;1 bearish</span>", _ => "0 neutral" };
                        t.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td></tr>", label, val, v));
                    }
                    let lean = if score > 0 { "bullish" } else if score < 0 { "bearish" } else { "neutral" };
                    t.push_str(&format!("<tr><td><b>composite</b></td><td><b>{:+}</b> of &minus;5..+5</td><td><b>{}</b></td></tr></table>", score, lean));
                    t
                }
                Err(e) => format!("<p class='note'>error: {}</p>", html_escape(&e)),
            };
            simple(200, "text/html; charset=utf-8", html.into_bytes())
        }
        // market price chart (real data; live=1 refreshes): close + SMA20 + SMA50 -> SVG
        ("POST", "/api/marketchart") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut live = false;
            let mut days = 180usize;
            let mut market = String::from("btc");
            for line in body.lines() {
                let mut it = line.splitn(2, '=');
                match (it.next(), it.next()) {
                    (Some("live"), Some(v)) => live = v.trim() == "1" || v.trim() == "true",
                    (Some("days"), Some(v)) => days = v.trim().parse().unwrap_or(days).clamp(20, 2000),
                    (Some("market"), Some(v)) => market = v.trim().to_lowercase(),
                    _ => {}
                }
            }
            let svg = crate::viz::market_chart_sym(&market, days, live);
            simple(200, "image/svg+xml; charset=utf-8", svg.into_bytes())
        }
        // the Latte ray tracer: body lines `w=N` `h=N` -> SVG raster + engine caption
        ("POST", "/api/trace") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut w = 96usize;
            let mut h = 72usize;
            let mut scene: Option<String> = None;
            for line in body.lines() {
                let mut it = line.splitn(2, '=');
                match (it.next().map(str::trim), it.next()) {
                    (Some("w"), Some(v)) => w = v.trim().parse().unwrap_or(w).clamp(16, 320),
                    (Some("h"), Some(v)) => h = v.trim().parse().unwrap_or(h).clamp(12, 240),
                    (Some("scene"), Some(v)) => scene = Some(v.trim().to_string()),
                    _ => {}
                }
            }
            let (mut svg, engine, ms) = crate::viz::ray_trace_scene(scene.as_deref(), w, h);
            // caption the render with its engine and time, inside the SVG itself
            if let Some(pos) = svg.rfind("</svg>") {
                let cap = format!(
                    "<text x='4' y='12' font-family='monospace' font-size='10' fill='#222' opacity='0.85'>lib/trace.lat — {}x{} in {} ms — {}</text>",
                    w, h, ms, engine
                );
                svg.insert_str(pos, &cap);
            }
            simple(200, "image/svg+xml; charset=utf-8", svg.into_bytes())
        }
        // live Ligurian derivation: body lines `mode=pie|solar|gen`, `n=`, `seed=`,
        // then words. Returns JSON rows {pie, solar, heart} — PIE -> Solar runs
        // lib/pie.sca, Solar -> Heart runs lib/ligurian.sca (+ prosodic passes),
        // and `gen` makes phonotactic Solar roots with lib/lexis.lat (in Latte).
        ("POST", "/api/derive") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut mode = "solar".to_string();
            let mut n = 10usize;
            let mut seed: u64 = 0;
            let mut words: Vec<String> = Vec::new();
            for line in body.lines() {
                let mut it = line.splitn(2, '=');
                match (it.next().map(str::trim), it.next()) {
                    (Some("mode"), Some(v)) => mode = v.trim().to_string(),
                    (Some("n"), Some(v)) => n = v.trim().parse().unwrap_or(n).clamp(1, 60),
                    (Some("seed"), Some(v)) => seed = v.trim().parse().unwrap_or(seed),
                    _ => words.extend(line.split_whitespace().map(|w| w.to_string())),
                }
            }
            let mut rows: Vec<(String, String, String)> = Vec::new();
            match mode.as_str() {
                "pie" => {
                    for w in words.iter().take(60) {
                        let solar = crate::sca::pie_to_solar(w).unwrap_or_else(|e| format!("(error: {})", e));
                        let heart = crate::sca::evolve(&solar).unwrap_or_else(|e| format!("(error: {})", e));
                        rows.push((w.clone(), solar, heart));
                    }
                }
                "gen" => {
                    if seed == 0 {
                        seed = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() ^ (d.subsec_nanos() as u64))
                            .unwrap_or(42) % 1_000_000;
                    }
                    for i in 0..n {
                        let expr = format!("(genword {})", seed + (i as u64) * 7919);
                        let word = match crate::rustgen::run_adaptive(&expr, &["std", "lexis"]) {
                            Ok(v) => cord_list_to_string(&v),
                            Err(e) => format!("(error: {})", e),
                        };
                        let heart = crate::sca::evolve(&word).unwrap_or_else(|e| format!("(error: {})", e));
                        rows.push((String::new(), word, heart));
                    }
                }
                _ => {
                    for w in words.iter().take(60) {
                        let heart = crate::sca::evolve(w).unwrap_or_else(|e| format!("(error: {})", e));
                        rows.push((String::new(), w.clone(), heart));
                    }
                }
            }
            let mut json = format!("{{\"mode\":\"{}\",\"seed\":{},\"rows\":[", json_escape(&mode), seed);
            for (i, (p, sol, h)) in rows.iter().enumerate() {
                if i > 0 { json.push(','); }
                json.push_str(&format!(
                    "{{\"pie\":\"{}\",\"solar\":\"{}\",\"heart\":\"{}\"}}",
                    json_escape(p), json_escape(sol), json_escape(h)
                ));
            }
            json.push_str("]}");
            simple(200, "application/json; charset=utf-8", json.into_bytes())
        }
        // the bundled real-news corpus, scored: JSON [{date,source,headline,polarity}..]
        ("GET", "/api/news") => {
            let items: Vec<(String, String, String)> = crate::marketdata::MARKET_NEWS
                .iter()
                .map(|(d, s, h)| (d.to_string(), s.to_string(), h.to_string()))
                .collect();
            let (rows, agg) = crate::numerics::score_news(&items);
            let mut json = String::from("{\"aggregate\":");
            json.push_str(&format!("{:.3},\"items\":[", agg));
            for (i, r) in rows.iter().enumerate() {
                if i > 0 { json.push(','); }
                json.push_str(&format!(
                    "{{\"date\":\"{}\",\"source\":\"{}\",\"headline\":\"{}\",\"polarity\":{:.3}}}",
                    r.date, json_escape(&r.source), json_escape(&r.headline), r.polarity
                ));
            }
            json.push_str("]}");
            simple(200, "application/json; charset=utf-8", json.into_bytes())
        }
        // sentiment scoring: body is the text (a headline OR a whole document/report);
        // returns JSON {positive,negative,lexicon,model,polarity,sentences:[{text,polarity}..]}
        // — the trained classifier scores sentence-by-sentence, so reports work too.
        ("POST", "/api/sentiment") => {
            let text = String::from_utf8_lossy(&req.body);
            let (pos, neg) = crate::sentiment::counts(&text);
            let lex = crate::sentiment::polarity(&text);
            let model = crate::sentiment::model_polarity(&text);
            let (doc, sents) = crate::sentiment::score_document(&text);
            let (dove, hawk) = crate::sentiment::rate_counts(&text);
            let bond = crate::sentiment::score_document_bond(&text).0;
            let mut json = format!(
                "{{\"positive\":{},\"negative\":{},\"lexicon\":{:.3},\"model\":{:.3},\"polarity\":{:.3},\"dovish\":{},\"hawkish\":{},\"bond_polarity\":{:.3},\"sentences\":[",
                pos, neg, lex, model, doc, dove, hawk, bond
            );
            for (i, (t, p)) in sents.iter().enumerate() {
                if i > 0 { json.push(','); }
                json.push_str(&format!("{{\"text\":\"{}\",\"polarity\":{:.3}}}", json_escape(t), p));
            }
            json.push_str("]}");
            simple(200, "application/json; charset=utf-8", json.into_bytes())
        }
        // multi-series line chart -> SVG. Body is line-oriented:
        //   line 1: title
        //   line 2: series labels separated by '|'
        //   each remaining non-empty line: one series of space-separated numbers
        ("POST", "/api/plotn") => {
            let body = String::from_utf8_lossy(&req.body);
            let mut lines = body.lines();
            let title = lines.next().unwrap_or("chart").trim().to_string();
            let labels: Vec<String> = lines
                .next()
                .unwrap_or("")
                .split('|')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let series: Vec<Vec<f64>> = lines
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.split_whitespace().filter_map(|t| t.parse::<f64>().ok()).collect())
                .collect();
            let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            simple(
                200,
                "image/svg+xml; charset=utf-8",
                crate::viz::render_lines(&title, &label_refs, &series).into_bytes(),
            )
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
        return "commands:\n  eval <expr>   run a Latte expression (std · mold · num · tensor · plan · ml · tool)\n  def NAME [args] = BODY   define a function for this session — then call it anywhere\n  undef NAME    remove a session-defined function (def alone lists them)\n  type <expr>   infer an expression's type\n  sca  <words>  evolve Solar words into Heart Speech (SCArs)\n  icomb         reduce interaction combinators (Lafont γ/δ/ε)\n  net <expr>    run an expression on the interaction net (lazy if, net recursion)\n  libs          list the libraries (modules) loaded in the running system\n  Module.cmd a  run arm `cmd` of a loaded Latte module on the argument(s) `a`"
            .into();
    }
    let (head, rest) = match cmd.split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim()),
        None => (cmd, ""),
    };
    match head {
        "eval" => eval_expr(rest),
        // one-line function definition, usable from any text (see latte::define_user_arm):
        //   def sq [x] = (mul x x)        then        eval (sq 7)
        "def" => match crate::latte::define_user_arm(rest) {
            Ok(m) | Err(m) => m,
        },
        "undef" => match crate::latte::undefine_user_arm(rest) {
            Ok(m) | Err(m) => m,
        },
        "type" => match latte::parse(rest) {
            Ok(ast) => match check::check(&ast) {
                Ok(ty) => format!("{} : {}", rest, ty.show()),
                // checker messages already carry their own "type error:" prefix
                Err(e) => {
                    let e = e.to_string();
                    if e.starts_with("type error") { e } else { format!("type error: {}", e) }
                }
            },
            Err(e) => format!("parse error: {}", e),
        },
        "icomb" => crate::icomb::demo(),
        // run an expression ON THE INTERACTION NET (lazy boxed `if`, net-level recursion,
        // γ-pairs; see docs/interaction-nets.md), cross-checked against the interpreter.
        "net" => match crate::icomb::run_str(rest) {
            Ok((v, steps)) => {
                let audit = crate::latte::run_with_libs(rest, &["std"])
                    .ok()
                    .and_then(|n| n.as_atom().and_then(|a| a.to_u128()));
                match audit {
                    Some(l) if l == v => format!("{} → {}  ({} interaction steps; interpreter agrees)", rest, v, steps),
                    Some(l) => format!("{} → {}  ({} steps) — INTERPRETER DISAGREES: {}", rest, v, steps, l),
                    None => format!("{} → {}  ({} interaction steps)", rest, v, steps),
                }
            }
            Err(e) => format!("net error: {}", e),
        },
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
            "unknown command '{}'. try: eval | type | sca | icomb | net | libs, or Module.command args",
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
    match crate::rustgen::run_adaptive(expr, &refs) {
        Ok(v) => render_result(&v),
        Err(e) => format!("error: {}", e),
    }
}

// ---- Forge: the team-coding state, hosted in this process -------------------
struct ForgeState {
    node: crate::net::Node,
    q: crate::mocha::Mocha,
}
fn forge_state() -> &'static std::sync::Mutex<Option<ForgeState>> {
    static F: std::sync::OnceLock<std::sync::Mutex<Option<ForgeState>>> = std::sync::OnceLock::new();
    F.get_or_init(|| std::sync::Mutex::new(None))
}
fn forge_init(slot: &mut Option<ForgeState>) -> Result<(), String> {
    if slot.is_some() {
        return Ok(());
    }
    let src = crate::mocha::FORGE_LAT;
    let agent = crate::agent::Agent::from_source(src, "forge").map_err(|e| e.to_string())?;
    let q = crate::mocha::Mocha::load(src)?;
    // the log persists beside the package directory: pkg/ and forge/ are siblings
    let dir = crate::latte::pkg_dir().with_file_name("forge");
    let node = match crate::net::Node::open(1, agent, dir.to_string_lossy().as_ref(), 0) {
        Ok(n) => n,
        Err(_) => {
            let agent2 = crate::agent::Agent::from_source(src, "forge").map_err(|e| e.to_string())?;
            crate::net::Node::new(1, agent2) // in-memory fallback
        }
    };
    *slot = Some(ForgeState { node, q });
    Ok(())
}

/// One forge command -> a plain-text reply. The List view prints each snippet
/// as a runnable `Forge.Open <name>` line, so the System's middle-click opens
/// it straight from the Log (the Oberon move).
fn forge_command(body: &str) -> String {
    let mut guard = forge_state().lock().unwrap();
    if let Err(e) = forge_init(&mut guard) {
        return format!("forge error: {}", e);
    }
    let st = guard.as_mut().unwrap();
    let (line, code) = match body.split_once('\n') {
        Some((l, c)) => (l.trim(), c),
        None => (body.trim(), ""),
    };
    let mut w = line.split_whitespace();
    let verb = w.next().unwrap_or("");
    let args: Vec<&str> = w.collect();
    use crate::knot::{cell, cord, num};
    let peek = |st: &ForgeState, tag: &str, arg: &str| -> Result<crate::knot::N, String> {
        let state = st.node.state().map_err(|e| format!("{:?}", e))?;
        let q_arg = if arg.is_empty() { num(0) } else { cord(arg) };
        st.q.peek(&cell(cord(tag), q_arg), &state).map_err(|e| format!("{:?}", e))
    };
    match verb {
        "share" => {
            let author = args.first().copied().unwrap_or("anon");
            let name = args.get(1).copied().unwrap_or("snippet");
            if code.trim().is_empty() {
                return "forge share: no code (the body's remaining lines are the snippet)".into();
            }
            let act = cell(cord("add"), cell(cord(author), cell(cord(name), cord(code))));
            st.node.local_action(act);
            let n = peek(st, "count", "")
                .ok()
                .and_then(|v| v.as_atom().and_then(|a| a.to_u128()))
                .unwrap_or(0);
            format!("shared '{}' as {} — {} snippet(s) on the forge", name, author, n)
        }
        "del" => match args.first() {
            Some(nm) => {
                st.node.local_action(cell(cord("del"), cord(nm)));
                format!("retired every '{}' snippet", nm)
            }
            None => "forge del: name a snippet".into(),
        },
        "clear" => {
            st.node.local_action(cell(cord("clear"), num(0)));
            "the forge is empty".into()
        }
        "get" => match args.first() {
            Some(nm) => match peek(st, "get", nm) {
                Ok(v) => match &*v {
                    Knot::Atom(a) if a.is_zero() => format!("(no snippet named '{}')", nm),
                    Knot::Cell(_, t) => match &**t {
                        Knot::Cell(_, code) => code
                            .as_atom()
                            .and_then(|a| a.as_text())
                            .unwrap_or_else(|| "(unreadable)".into()),
                        _ => "(unreadable)".into(),
                    },
                    _ => "(unreadable)".into(),
                },
                Err(e) => format!("forge error: {}", e),
            },
            None => "forge get: name a snippet".into(),
        },
        "list" | "" | "all" | "by" => {
            let (tag, arg) = if verb == "by" {
                ("by", args.first().copied().unwrap_or(""))
            } else {
                ("all", "")
            };
            match peek(st, tag, arg) {
                Ok(mut v) => {
                    let mut out = Vec::new();
                    while let Knot::Cell(h, t) = &*v.clone() {
                        if let Knot::Cell(author, nt) = &**h {
                            if let Knot::Cell(name, code) = &**nt {
                                let nm = name.as_atom().and_then(|a| a.as_cord()).unwrap_or_default();
                                let au = author.as_atom().and_then(|a| a.as_cord()).unwrap_or_default();
                                let lines = code
                                    .as_atom()
                                    .and_then(|a| a.as_text())
                                    .map(|c| c.lines().count())
                                    .unwrap_or(0);
                                out.push(format!("Forge.Open {}   :: by {} — {} line(s)", nm, au, lines));
                            }
                        }
                        v = t.clone();
                    }
                    if out.is_empty() {
                        "the forge is empty — share the marked frame with Forge.Share".into()
                    } else {
                        out.join("\n")
                    }
                }
                Err(e) => format!("forge error: {}", e),
            }
        }
        "names" => match peek(st, "names", "") {
            Ok(mut v) => {
                let mut out = Vec::new();
                while let Knot::Cell(h, t) = &*v.clone() {
                    if let Some(s) = h.as_atom().and_then(|a| a.as_cord()) {
                        out.push(s);
                    }
                    v = t.clone();
                }
                if out.is_empty() { "(none)".into() } else { out.join(" ") }
            }
            Err(e) => format!("forge error: {}", e),
        },
        "count" => peek(st, "count", "")
            .ok()
            .and_then(|v| v.as_atom().and_then(|a| a.to_u128()))
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".into()),
        other => format!("forge: unknown verb '{}' (share · list · names · get · by · del · clear · count)", other),
    }
}

/// Render an eval result. A cell tagged [%svg cord] or [%html cord] is a RENDER
/// OBJECT: a pure-Latte tool's way to return live markup. The payload is sent
/// behind a \u{1}kind\u{1} marker; the System embeds it as an object (the same
/// way chart/gfx output embeds), so text-producing tools need no Rust at all.
pub(crate) fn render_result(n: &N) -> String {
    if let Knot::Cell(h, t) = &**n {
        if let (Knot::Atom(tag), Knot::Atom(payload)) = (&**h, &**t) {
            if let Some(kind) = tag.as_cord() {
                if kind == "svg" || kind == "html" {
                    if let Some(s) = payload.as_text() {
                        return format!("\u{1}{}\u{1}{}", kind, s);
                    }
                }
            }
        }
    }
    render_noun(n)
}

/// Render a noun for display: small atoms as decimals, multi-byte printable atoms as text
/// (cords), and cells as `[a b]`. (Every atom is both a number and a possible string; in an
/// eval console the number is the expected reading, e.g. `(mul 6 7)` is `42`, not `"*"`.)
pub(crate) fn render_noun(n: &N) -> String {
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
    if !(name.ends_with(".facet") || name.ends_with(".md")) {
        return None;
    }
    Some(std::path::Path::new(root).join(name))
}

/// The library directory that holds the system's `*.lat` source. The GUI root is the site
/// directory (`lib/site`); the libraries sit one level up (`lib`).
fn lib_dir(root: &str) -> Option<std::path::PathBuf> {
    std::path::Path::new(root).parent().map(|p| p.to_path_buf())
}

/// Run the `lookup` library (lk_lookup) — the Latte definition-lookup tool — over
/// every module's source and return the first definition found, prefixed with the
/// module it lives in. The host's only job is to split each module into line cords,
/// hand them to the Latte tool, and render the line cords it hands back.
fn lookup_definition(name: &str, root: &str) -> String {
    // gather module names in LOAD ORDER (the eval scope's concatenation order, so a
    // later library shadows an earlier one), then any disk libs not already present.
    let mut names: Vec<String> = crate::latte::all_libs();
    {
        let mut seen: std::collections::HashSet<String> = names.iter().cloned().collect();
        if let Some(dir) = lib_dir(root) {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("lat") {
                        if let Some(stem) = p.file_stem().and_then(|x| x.to_str()) {
                            if seen.insert(stem.to_string()) {
                                names.push(stem.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // a Latte string-literal for a single source line (escape \  "  tab)
    fn quote_line(line: &str) -> String {
        let mut q = String::with_capacity(line.len() + 2);
        q.push('"');
        for ch in line.chars() {
            match ch {
                '\\' => q.push_str("\\\\"),
                '"' => q.push_str("\\\""),
                '\t' => q.push_str("\\t"),
                _ => q.push(ch),
            }
        }
        q.push('"');
        q
    }

    // does `modname` define a top-level arm `name`? (the `  name` + space/'=' prefilter)
    let module_defines = |modname: &str| -> bool {
        crate::latte::library_source(modname)
            .map(|src| {
                src.lines().any(|l| {
                    l.strip_prefix("  ")
                        .and_then(|r| r.strip_prefix(name))
                        .map(|rest| rest.starts_with(' ') || rest.starts_with('='))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };

    // every module that defines the name, in load order; the LAST one wins in the
    // shared flat scope, so report the others as shadowed (the database-backed
    // Symbols tool, lib/symbols.lat, makes the same fact queryable in-system).
    let definers: Vec<String> = names.into_iter().filter(|m| module_defines(m)).collect();
    let winner = match definers.last() {
        Some(w) => w.clone(),
        None => return format!("no definition found for '{}'", name),
    };
    let header = if definers.len() > 1 {
        format!(
            "{} is defined in {} modules: {}\n{} wins (loaded last); the rest are shadowed in the shared scope.\n\n",
            name,
            definers.len(),
            definers.join(", "),
            winner
        )
    } else {
        String::new()
    };

    // extract the winning definition with the lookup tool (lk_lookup over its source)
    let src = match crate::latte::library_source(&winner) {
        Some(s) => s,
        None => return format!("no definition found for '{}'", name),
    };
    let mut list = String::from("0");
    for line in src.lines().rev() {
        list = format!("[ {} {} ]", quote_line(line), list);
    }
    let expr = format!("(lk_lookup {} {})", list, quote_line(name));
    let n = match crate::rustgen::run_adaptive(&expr, &["std", "lookup"]) {
        Ok(n) => n,
        Err(e) => return format!("lookup error: {}", e),
    };
    let mut out = String::new();
    let mut cur = &n;
    while let Some((head, tail)) = cur.as_cell() {
        if let Some(a) = head.as_atom() {
            out.push_str(&String::from_utf8_lossy(a.bytes_le()));
            out.push('\n');
        }
        cur = tail;
    }
    format!("{}module {}\n\n{}", header, winner, out.trim_end())
}

/// Build the symbol-index triples `[ [ %mod [ %name [ arity 0 ] ] ] .. 0 ]` for just
/// the modules that define `name` — the system hands that small slice of its surface
/// to the database (lib/symbols.lat), which indexes and renders it. Scoping to one
/// name keeps the per-request index tiny, so the immutable db build stays cheap.
fn name_symbol_triples(name: &str) -> String {
    fn arm_arity(body: &str) -> usize {
        let b = body.trim_start();
        if let Some(rest) = b.strip_prefix("fn ").map(|r| r.trim_start()) {
            if let Some(inner) = rest.strip_prefix('[') {
                if let Some(end) = inner.find(']') {
                    return inner[..end].split_whitespace().count();
                }
            }
        }
        0
    }
    let head = format!("  {} = ", name);
    let mut items: Vec<(String, usize)> = Vec::new();
    for modname in crate::latte::all_libs() {
        let src = match crate::latte::library_source(&modname) {
            Some(s) => s,
            None => continue,
        };
        for line in src.lines() {
            if let Some(body) = line.strip_prefix(&head) {
                items.push((modname.clone(), arm_arity(body)));
                break; // one definition per module
            }
        }
    }
    let mut triples = String::from("0");
    for (m, ar) in items.iter().rev() {
        triples = format!("[ [ %{} [ %{} [ {} 0 ] ] ] {} ]", m, name, ar, triples);
    }
    triples
}

/// The `[%html cord]` body of a render result (or None if the noun isn't tagged html).
fn extract_html(n: &N) -> Option<String> {
    if let Knot::Cell(h, t) = &**n {
        if let (Knot::Atom(tag), Knot::Atom(body)) = (&**h, &**t) {
            if tag.as_cord().as_deref() == Some("html") {
                if let Some(s) = body.as_text() {
                    return Some(s);
                }
            }
        }
    }
    None
}

/// The database-backed symbol report for `name`: index its definers with
/// lib/symbols.lat (built on the composed database) and render the result.
fn symbol_index_html(name: &str) -> String {
    let triples = name_symbol_triples(name);
    if triples == "0" {
        return format!("<p>no loaded module defines <code>{}</code></p>", html_escape(name));
    }
    let expr = format!("(sy_html (sy_build {}) %{})", triples, name);
    let libs = crate::latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    match crate::rustgen::run_adaptive(&expr, &refs) {
        Ok(n) => extract_html(&n)
            .unwrap_or_else(|| format!("<p>no symbol named <code>{}</code></p>", html_escape(name))),
        Err(e) => format!("<p>symbol lookup error: {}</p>", html_escape(&e)),
    }
}

/// A database-backed price dashboard for the last `n` bars of `market`: load the
/// window into lib/findb.lat's store (on the composed database), then read it back
/// for the sparkline + statistics + the fitted lag-1 model it renders as [%html].
fn findb_dash_html(market: &str, n: usize) -> String {
    let n = n.clamp(8, 60);
    let (closes, _span, label) = match crate::marketdata::closes_market(market, false) {
        Ok(v) => v,
        Err(e) => return format!("<p>no market data for <code>{}</code>: {}</p>",
            html_escape(market), html_escape(&e)),
    };
    if closes.len() < 3 {
        return "<p>not enough price history to analyse</p>".to_string();
    }
    let n = n.min(closes.len());
    let window = &closes[closes.len() - n..];
    // a Latte list of n-values [0 magnitude]; the embedded series is ×100, the
    // num library is ×1000, so scale each close up by 10.
    let mut lit = String::from("0");
    for &c in window.iter().rev() {
        let mag = c.max(0) * 10;
        lit = format!("[ [0 {}] {} ]", mag, lit);
    }
    let expr = format!("(fd_dash (fd_load {}) {})", lit, n);
    let libs = crate::latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    let body = match crate::rustgen::run_adaptive(&expr, &refs) {
        Ok(nn) => extract_html(&nn)
            .unwrap_or_else(|| "<p>could not render the price dashboard</p>".to_string()),
        Err(e) => format!("<p>findb error: {}</p>", html_escape(&e)),
    };
    format!("<p style=\"font:600 13px system-ui;margin:.1em 0\">{}</p>{}", html_escape(&label), body)
}

/// The documentation directory (`docs/`), a sibling of the library directory.
/// The user-text directory: <root>/../text (a sibling of lib/site), i.e. the
/// distribution's text/ folder — documents with embedded objects live here.
fn list_ext(dir: &std::path::Path, ext: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Some(n) = e.file_name().to_str() {
                if let Some(stem) = n.strip_suffix(&format!(".{}", ext)) {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort();
    names
}

fn drawings_dir(root: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(root).parent().and_then(|p| p.parent()).map(|p| p.join("drawings"));
    match p {
        Some(d) if !d.as_os_str().is_empty() => Some(d),
        _ => Some(std::path::PathBuf::from("drawings")),
    }
}

fn text_dir(root: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(root).parent().and_then(|p| p.parent()).map(|p| p.join("text"));
    match p {
        Some(d) if !d.as_os_str().is_empty() => Some(d),
        _ => Some(std::path::PathBuf::from("text")),
    }
}

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
/// A safe database name: identifier-ish, used as a WAL filename.
fn is_db_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

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

// ----- writing --------------------------------------------------------------
// ----- small helpers --------------------------------------------------------
/// Decode a ui.lat panel noun to JSON for the GUI renderer.
fn panel_json(n: &crate::knot::N) -> Option<String> {
    fn cord(n: &crate::knot::N) -> Option<String> {
        n.as_atom().and_then(|a| a.as_cord())
    }
    fn cell(n: &crate::knot::N) -> Option<(crate::knot::N, crate::knot::N)> {
        if let Knot::Cell(h, t) = &**n { Some((h.clone(), t.clone())) } else { None }
    }
    fn list(mut n: crate::knot::N) -> Vec<crate::knot::N> {
        let mut out = Vec::new();
        while let Knot::Cell(h, t) = &*n.clone() {
            out.push(h.clone());
            n = t.clone();
        }
        out
    }
    fn item(n: &crate::knot::N) -> Option<String> {
        let (tag, payload) = cell(n)?;
        let t = cord(&tag)?;
        match t.as_str() {
            "label" => {
                let (text, _) = cell(&payload)?;
                Some(format!("{{\"t\":\"label\",\"text\":\"{}\"}}", json_escape(&cord(&text)?)))
            }
            "field" => {
                let (name, rest) = cell(&payload)?;
                let (init, _) = cell(&rest)?;
                Some(format!(
                    "{{\"t\":\"field\",\"name\":\"{}\",\"init\":\"{}\"}}",
                    json_escape(&cord(&name)?),
                    json_escape(&cord(&init).or_else(|| init.as_atom().map(|a| a.to_u128().map(|v| v.to_string()).unwrap_or_default()))?)
                ))
            }
            "button" => {
                let (text, rest) = cell(&payload)?;
                let (cmdwords, _) = cell(&rest)?;
                let words: Vec<String> = list(cmdwords).iter().filter_map(cord).collect();
                Some(format!(
                    "{{\"t\":\"button\",\"text\":\"{}\",\"cmd\":\"{}\"}}",
                    json_escape(&cord(&text)?),
                    json_escape(&words.join(" "))
                ))
            }
            "row" => {
                let (items, _) = cell(&payload)?;
                let parts: Vec<String> = list(items).iter().filter_map(item).collect();
                Some(format!("{{\"t\":\"row\",\"items\":[{}]}}", parts.join(",")))
            }
            _ => None,
        }
    }
    let (tag, payload) = cell(n)?;
    if cord(&tag)? != "panel" {
        return None;
    }
    let (title, rest) = cell(&payload)?;
    let (items, _) = cell(&rest)?;
    let parts: Vec<String> = list(items).iter().filter_map(|i| item(i)).collect();
    Some(format!(
        "{{\"title\":\"{}\",\"items\":[{}]}}",
        json_escape(&cord(&title)?),
        parts.join(",")
    ))
}

/// Join a 0-terminated list of cords into a string, skipping the "_" marker
/// (lib/lexis.lat uses %_ for an empty syllable coda).
fn cord_list_to_string(n: &crate::knot::N) -> String {
    let mut out = String::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur.clone() {
        if let Some(c) = h.as_atom().and_then(|a| a.as_cord()) {
            if c != "_" {
                out.push_str(&c);
            }
        }
        cur = t.clone();
    }
    out
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Format unix seconds as an RFC 1123 HTTP date (always GMT).

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
    fn api_eval_runs_a_live_expression() {
        // the /api/eval endpoint backs Live.box: evaluate `expr` against posted inputs
        let mut h = std::collections::HashMap::new();
        let req = Request {
            method: "POST".into(),
            path: "/api/eval".into(),
            version: "HTTP/1.1".into(),
            headers: { h.insert("content-type".into(), "application/x-www-form-urlencoded".into()); h },
            keep_alive: true,
            // expr=SCArs.apply(word, Txt.split(rules, ";")) with word=kasa rules=k>g; a>o
            body: "expr=SCArs.apply(word%2C%20Txt.split(rules%2C%20%22%3B%22))&word=kasa&rules=k%3Eg%3B%20a%3Eo"
                .as_bytes()
                .to_vec(),
            query: String::new(),
        };
        let r = api_handle(&req, &None, &None, ".");
        assert_eq!(r.status, 200);
        assert_eq!(String::from_utf8_lossy(&r.body), "goso");
        // a repeated identical request is served from the memo cache (same result)
        let r2 = api_handle(&req, &None, &None, ".");
        assert_eq!(r2.status, 200);
        assert_eq!(String::from_utf8_lossy(&r2.body), "goso");
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
