use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use regex::Regex;
use serde_json::{json, Value};

/// SQL*Plus / SQLcl directive keywords that plsqllang-server's parser doesn't
/// recognize and reports as e.g. mismatched input 'VERIFY' expecting {...}.
/// Add more keywords here as you find them — no other code needs to change.
const SQLPLUS_DIRECTIVE_TOKENS: &[&str] = &[
    "VERIFY",
    "ECHO",
    "DEFINE",
    "FEEDBACK",
    "HEADING",
    "LINESIZE",
    "PAGESIZE",
    "TIMING",
    "TERMOUT",
    "TRIMSPOOL",
    "SQLBLANKLINES",
    "AUTOCOMMIT",
    "SCAN",
    "SQLPROMPT",
    "WRAP",
    "LONG",
    "ARRAYSIZE",
    "NUMWIDTH",
    "TAB",
    "SERVEROUTPUT",
];

/// plsqllang-server's grammar only expects a single top-level statement per
/// document, so a multi-statement script (several CREATE/DECLARE blocks
/// separated by `/`) only ever gets the first statement checked. To work
/// around this, this proxy splits every document into one virtual
/// "chunk" document per top-level statement, sends each chunk to the real
/// server under its own synthetic URI, and remaps + merges the diagnostics
/// it sends back onto the real document's line numbers.
///
/// This only fixes textDocument/publishDiagnostics. Requests like hover or
/// go-to-definition still reference positions in the real document, which
/// the child server has no notion of (it only ever sees chunk-local
/// documents) — those are forwarded unchanged and may not resolve
/// correctly. If you don't rely on those for .sql files, this is fine.
#[derive(Clone)]
struct Chunk {
    uri: String,
    start_line: u32,
    diagnostics: Vec<Value>,
}

#[derive(Clone)]
struct DocumentState {
    version: i64,
    language_id: String,
    chunks: Vec<Chunk>,
}

struct ProxyState {
    /// real document uri -> its current chunk breakdown
    documents: HashMap<String, DocumentState>,
    /// synthetic chunk uri -> (real document uri, index into that document's chunks)
    chunk_owner: HashMap<String, (String, usize)>,
    /// id of the client's "initialize" request, so we can patch the
    /// matching response from the real server when it comes back.
    pending_initialize_id: Option<Value>,
    /// monotonically increasing counter used to build unique chunk uris.
    next_chunk_seq: u64,
}

impl ProxyState {
    fn new() -> Self {
        ProxyState {
            documents: HashMap::new(),
            chunk_owner: HashMap::new(),
            pending_initialize_id: None,
            next_chunk_seq: 0,
        }
    }
}

type SharedState = Arc<Mutex<ProxyState>>;
type SharedWriter<W> = Arc<Mutex<W>>;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "plsqllang-proxy: no command given. Usage: plsqllang-proxy <real-server-command> [args...]"
        );
        std::process::exit(1);
    }

    let real_command = &args[0];
    let real_args = &args[1..];

    let mut child = Command::new(real_command)
        .args(real_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("plsqllang-proxy: failed to start '{}': {}", real_command, e);
            std::process::exit(1);
        });

    let child_stdin = child.stdin.take().expect("child stdin");
    let child_stdout = child.stdout.take().expect("child stdout");

    let state: SharedState = Arc::new(Mutex::new(ProxyState::new()));
    let child_stdin: SharedWriter<_> = Arc::new(Mutex::new(child_stdin));

    // Thread: read Zed's requests/notifications from our stdin, rewrite
    // document-sync notifications into per-chunk virtual documents, and
    // forward everything (rewritten or as-is) to the real server's stdin.
    let client_thread = {
        let state = Arc::clone(&state);
        let child_stdin = Arc::clone(&child_stdin);
        thread::spawn(move || {
            let mut reader = BufReader::new(io::stdin());
            loop {
                match read_message(&mut reader) {
                    Some(body) => handle_client_message(&body, &state, &child_stdin),
                    None => break, // EOF: Zed closed our stdin.
                }
            }
        })
    };

    let patterns = build_patterns();

    // Main thread: read the real server's responses/notifications, remap
    // and merge per-chunk diagnostics back onto the real document, and
    // forward everything else unchanged to Zed (our stdout).
    let mut reader = BufReader::new(child_stdout);
    let stdout = io::stdout();

    loop {
        match read_message(&mut reader) {
            Some(body) => handle_server_message(&body, &state, &patterns, &stdout),
            None => break, // EOF: real server closed its stdout.
        }
    }

    client_thread.join().ok();
    let _ = child.wait();
}

/// Builds the list of regexes used to identify known false-positive
/// diagnostics from plsqllang-server.
fn build_patterns() -> Vec<Regex> {
    // Matches: mismatched input 'VERIFY' expecting {...}
    // for any keyword in SQLPLUS_DIRECTIVE_TOKENS, case-insensitive.
    let tokens = SQLPLUS_DIRECTIVE_TOKENS.join("|");
    let token_pattern = format!(r"(?i)mismatched input '(?:{})'\s+expecting", tokens);

    // Matches: mismatched input '&emp_id' expecting {...}
    // i.e. any SQL*Plus substitution variable reference like &foo.
    let substitution_pattern = r"mismatched input '&\w+'".to_string();

    vec![
        Regex::new(&token_pattern).expect("valid regex"),
        Regex::new(&substitution_pattern).expect("valid regex"),
    ]
}

// ---------------------------------------------------------------------
// Client -> server direction
// ---------------------------------------------------------------------

fn handle_client_message<W: Write>(
    body: &[u8],
    state: &SharedState,
    child_stdin: &SharedWriter<W>,
) {
    let Ok(json_val): Result<Value, _> = serde_json::from_slice(body) else {
        forward_raw(body, child_stdin);
        return;
    };

    let method = json_val.get("method").and_then(Value::as_str).unwrap_or("");

    match method {
        "initialize" => {
            if let Some(id) = json_val.get("id").cloned() {
                let mut st = state.lock().unwrap();
                st.pending_initialize_id = Some(id);
            }
            forward_value(&json_val, child_stdin);
        }
        "textDocument/didOpen" => {
            if let Some(text_document) = json_val.get("params").and_then(|p| p.get("textDocument"))
            {
                let uri = text_document
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let language_id = text_document
                    .get("languageId")
                    .and_then(Value::as_str)
                    .unwrap_or("sql")
                    .to_string();
                let version = text_document
                    .get("version")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let text = text_document
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                open_document(&uri, language_id, version, &text, state, child_stdin);
            }
            // Deliberately not forwarding the original didOpen: the real
            // server only ever sees the per-chunk virtual documents, never
            // the real uri.
        }
        "textDocument/didChange" => {
            if let Some(params) = json_val.get("params") {
                let uri = params
                    .get("textDocument")
                    .and_then(|t| t.get("uri"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let version = params
                    .get("textDocument")
                    .and_then(|t| t.get("version"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);

                // Assumes full-document sync: contentChanges' last entry's
                // "text" is the whole new document. We force the server to
                // advertise Full sync in its initialize response (see
                // force_full_text_sync below) so Zed always sends whole-file
                // text on every edit, making this assumption safe.
                let text = params
                    .get("contentChanges")
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.last())
                    .and_then(|c| c.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();

                let language_id = {
                    let st = state.lock().unwrap();
                    st.documents
                        .get(&uri)
                        .map(|d| d.language_id.clone())
                        .unwrap_or_else(|| "sql".to_string())
                };

                close_document(&uri, state, child_stdin);
                open_document(&uri, language_id, version, &text, state, child_stdin);
            }
        }
        "textDocument/didClose" => {
            if let Some(uri) = json_val
                .get("params")
                .and_then(|p| p.get("textDocument"))
                .and_then(|t| t.get("uri"))
                .and_then(Value::as_str)
            {
                close_document(uri, state, child_stdin);
            }
        }
        _ => {
            forward_value(&json_val, child_stdin);
        }
    }
}

/// Splits `text` into one virtual chunk document per top-level statement and
/// sends each as its own textDocument/didOpen to the real server.
fn open_document<W: Write>(
    uri: &str,
    language_id: String,
    version: i64,
    text: &str,
    state: &SharedState,
    child_stdin: &SharedWriter<W>,
) {
    let chunk_texts = split_into_chunks(text);

    let mut new_chunks = Vec::new();
    {
        let mut st = state.lock().unwrap();
        for (_chunk_text, start_line) in &chunk_texts {
            st.next_chunk_seq += 1;
            let chunk_uri = format!("{}#plsqllang-proxy-chunk-{}", uri, st.next_chunk_seq);
            new_chunks.push(Chunk {
                uri: chunk_uri,
                start_line: *start_line,
                diagnostics: Vec::new(),
            });
        }
        for (idx, chunk) in new_chunks.iter().enumerate() {
            st.chunk_owner
                .insert(chunk.uri.clone(), (uri.to_string(), idx));
        }
        // Insert the document *before* any didOpen is sent below: the
        // real server can reply to the very first chunk before we've
        // finished sending the rest, and that response needs somewhere
        // to land the moment it arrives, not after the whole loop below
        // finishes. Populating chunk_owner and documents together here,
        // under one lock, closes that race.
        st.documents.insert(
            uri.to_string(),
            DocumentState {
                version,
                language_id: language_id.clone(),
                chunks: new_chunks.clone(),
            },
        );
    }

    for (chunk, (chunk_text, _start_line)) in new_chunks.iter().zip(chunk_texts.iter()) {
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": chunk.uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": chunk_text,
                }
            }
        });
        forward_value(&notif, child_stdin);
    }
}

/// Closes all of a document's current virtual chunks (sends didClose for
/// each) and forgets them, ahead of either a real close or a full re-split
/// on edit.
fn close_document<W: Write>(uri: &str, state: &SharedState, child_stdin: &SharedWriter<W>) {
    let old_chunks = {
        let mut st = state.lock().unwrap();
        let chunks = st
            .documents
            .remove(uri)
            .map(|d| d.chunks)
            .unwrap_or_default();
        for chunk in &chunks {
            st.chunk_owner.remove(&chunk.uri);
        }
        chunks
    };

    for chunk in old_chunks {
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": chunk.uri } }
        });
        forward_value(&notif, child_stdin);
    }
}

/// Splits a document into one chunk per top-level SQL/PL-SQL statement.
///
/// PL/SQL program units — CREATE [OR REPLACE] PROCEDURE/FUNCTION/PACKAGE
/// [BODY]/TRIGGER, and anonymous DECLARE/BEGIN blocks — are terminated by a
/// "/" on its own line, per SQL*Plus/SQLcl convention; a ';' inside one is
/// just part of the PL/SQL body, not a statement end. Everything else
/// (CREATE TABLE, CREATE INDEX, ALTER, GRANT, plain INSERT/UPDATE/DELETE,
/// etc.) is a single statement terminated by the first top-level ';' —
/// outside any parentheses, quoted string, or comment. A stray "/" after a
/// plain statement (harmless in real scripts) just closes an already-empty
/// chunk and is dropped.
///
/// This is a heuristic, not a full PL/SQL parser: it classifies a
/// statement as a PL/SQL unit by scanning its accumulated text so far for
/// whole-word PROCEDURE/FUNCTION/PACKAGE/TRIGGER (or TYPE ... BODY) after
/// CREATE, or a leading DECLARE/BEGIN. It's good enough for real scripts,
/// but e.g. a plain statement whose only mention of one of those words is
/// deep inside a string literal or comment won't confuse it (strings and
/// comments are tracked and skipped), while a genuinely unusual statement
/// shape could in principle misclassify.
///
/// Returns (chunk_text, start_line) pairs, where start_line is the 0-based
/// line number of the chunk's first line in the original document.
fn split_into_chunks(text: &str) -> Vec<(String, u32)> {
    let bytes = text.as_bytes();
    let len = bytes.len();

    let mut chunks = Vec::new();
    let mut i: usize = 0;
    let mut stmt_start: usize = 0;
    let mut line: u32 = 0;
    let mut stmt_start_line: u32 = 0;

    let mut paren_depth: i32 = 0;
    let mut in_single_quote = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    // None until we've seen enough of the statement to classify it; then
    // locks in for the rest of the statement.
    let mut is_plsql_unit: Option<bool> = None;

    while i < len {
        let c = bytes[i] as char;

        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                line += 1;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == '*' && i + 1 < len && bytes[i + 1] as char == '/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            if c == '\n' {
                line += 1;
            }
            i += 1;
            continue;
        }
        if in_single_quote {
            if c == '\'' {
                if i + 1 < len && bytes[i + 1] as char == '\'' {
                    // doubled '' is an escaped quote inside the string
                    i += 2;
                    continue;
                }
                in_single_quote = false;
            }
            if c == '\n' {
                line += 1;
            }
            i += 1;
            continue;
        }

        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            '\'' => {
                in_single_quote = true;
                i += 1;
            }
            '-' if i + 1 < len && bytes[i + 1] as char == '-' => {
                in_line_comment = true;
                i += 2;
            }
            '/' if i + 1 < len && bytes[i + 1] as char == '*' => {
                in_block_comment = true;
                i += 2;
            }
            '(' => {
                paren_depth += 1;
                i += 1;
            }
            ')' => {
                paren_depth -= 1;
                i += 1;
            }
            '/' if is_line_only_slash(bytes, i, len) => {
                // A lone "/" is an explicit, unambiguous terminator — split
                // here even if paren_depth is stuck above zero (e.g. from
                // an unbalanced paren earlier in this statement, which is
                // itself a genuine syntax error). Not resetting paren_depth
                // would let one broken statement poison every statement
                // after it for the rest of the file.
                push_chunk_range(&mut chunks, text, stmt_start, i, stmt_start_line);
                let mut j = i + 1;
                while j < len && bytes[j] as char != '\n' {
                    j += 1;
                }
                if j < len {
                    j += 1;
                    line += 1;
                }
                i = j;
                stmt_start = i;
                stmt_start_line = line;
                is_plsql_unit = None;
                paren_depth = 0;
            }
            ';' if paren_depth == 0 && is_plsql_unit != Some(true) => {
                if is_plsql_unit.is_none() {
                    is_plsql_unit = Some(looks_like_plsql_unit(&text[stmt_start..=i]));
                }
                if is_plsql_unit == Some(false) {
                    push_chunk_range(&mut chunks, text, stmt_start, i + 1, stmt_start_line);
                    i += 1;
                    stmt_start = i;
                    stmt_start_line = line;
                    is_plsql_unit = None;
                    // Defensive reset, mirroring the "/" branch above: a
                    // well-formed statement can only get here with
                    // paren_depth already at 0, but if a future change adds
                    // another split trigger, or a stray unmatched ')' ever
                    // drove paren_depth negative, leaving it un-reset would
                    // let this statement's parenthesis bookkeeping poison
                    // every statement after it for the rest of the file.
                    paren_depth = 0;
                    continue;
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }

        if is_plsql_unit.is_none() && i > stmt_start && looks_like_plsql_unit(&text[stmt_start..i])
        {
            is_plsql_unit = Some(true);
        }
    }

    if stmt_start < len {
        push_chunk_range(&mut chunks, text, stmt_start, len, stmt_start_line);
    }

    chunks
}

/// True if the '/' at `bytes[pos]` is alone on its line (only whitespace
/// before it up to the preceding newline, and only whitespace — or a
/// trailing "-- ..." line comment — after it up to the following newline).
/// This is the SQL*Plus statement-terminator convention, as opposed to
/// e.g. division or the start of a "/*" block comment (already handled
/// separately). SQL*Plus scripts commonly annotate the terminator with a
/// trailing comment (e.g. "/  -- end of proc_x"), so that must not
/// disqualify it — treating it as an ordinary character instead would
/// silently fail to split there and merge every statement after it into
/// one oversized chunk for the rest of the file.
fn is_line_only_slash(bytes: &[u8], pos: usize, len: usize) -> bool {
    let mut j = pos;
    while j > 0 && bytes[j - 1] as char != '\n' {
        if !(bytes[j - 1] as char).is_whitespace() {
            return false;
        }
        j -= 1;
    }
    let mut k = pos + 1;
    while k < len && (bytes[k] as char).is_whitespace() && bytes[k] as char != '\n' {
        k += 1;
    }
    if k < len && bytes[k] as char != '\n' {
        // The only content allowed after trailing whitespace is a line
        // comment running to the end of the line.
        let is_line_comment_start =
            bytes[k] as char == '-' && k + 1 < len && bytes[k + 1] as char == '-';
        if !is_line_comment_start {
            return false;
        }
    }
    true
}

/// Heuristically classifies accumulated statement text as a PL/SQL program
/// unit (needs a trailing "/") based on whole-word keywords, robust to
/// arbitrary internal whitespace/newlines and to substrings like
/// "PROCEDURE_ID" that merely contain a keyword.
fn looks_like_plsql_unit(stmt_so_far: &str) -> bool {
    let normalized = format!(
        " {} ",
        stmt_so_far
            .to_uppercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    );
    if normalized.starts_with(" DECLARE ") || normalized.starts_with(" BEGIN ") {
        return true;
    }
    if normalized.contains(" CREATE ") {
        return normalized.contains(" PROCEDURE ")
            || normalized.contains(" FUNCTION ")
            || normalized.contains(" PACKAGE ")
            || normalized.contains(" TRIGGER ")
            || (normalized.contains(" TYPE ") && normalized.contains(" BODY "));
    }
    false
}

/// Pushes text[start..end] as a chunk, unless it's empty or contains only
/// blank lines / line comments (nothing worth sending to the real server).
fn push_chunk_range(
    chunks: &mut Vec<(String, u32)>,
    text: &str,
    start: usize,
    end: usize,
    start_line: u32,
) {
    if start >= end {
        return;
    }
    let slice = &text[start..end];
    let is_empty = slice.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with("--")
    });
    if is_empty {
        return;
    }
    chunks.push((slice.to_string(), start_line));
}

// ---------------------------------------------------------------------
// Server -> client direction
// ---------------------------------------------------------------------

fn handle_server_message(
    body: &[u8],
    state: &SharedState,
    patterns: &[Regex],
    stdout: &io::Stdout,
) {
    let Ok(mut json_val): Result<Value, _> = serde_json::from_slice(body) else {
        let mut handle = stdout.lock();
        write_message(&mut handle, body);
        return;
    };

    // Patch the initialize response so the client always sends full
    // document text on every edit. This lets us always re-split and
    // re-send whole documents on didChange rather than tracking
    // incremental deltas against chunk boundaries.
    if let Some(id) = json_val.get("id").cloned() {
        let mut st = state.lock().unwrap();
        if st.pending_initialize_id.as_ref() == Some(&id) {
            st.pending_initialize_id = None;
            drop(st);
            force_full_text_sync(&mut json_val);
        }
    }

    let is_publish_diagnostics = json_val
        .get("method")
        .and_then(Value::as_str)
        .map(|m| m == "textDocument/publishDiagnostics")
        .unwrap_or(false);

    if is_publish_diagnostics {
        handle_chunk_diagnostics(json_val, state, patterns, stdout);
        return;
    }

    let bytes = serde_json::to_vec(&json_val).unwrap_or_else(|_| body.to_vec());
    let mut handle = stdout.lock();
    write_message(&mut handle, &bytes);
}

fn force_full_text_sync(response: &mut Value) {
    if let Some(caps) = response
        .get_mut("result")
        .and_then(|r| r.get_mut("capabilities"))
    {
        caps["textDocumentSync"] = json!(1); // TextDocumentSyncKind::Full
    }
}

/// Handles a publishDiagnostics notification for one of our synthetic chunk
/// uris: filters known false positives, remaps line numbers back onto the
/// real document, merges with whatever the document's other chunks last
/// reported, and emits a single publishDiagnostics for the real uri.
fn handle_chunk_diagnostics(
    json_val: Value,
    state: &SharedState,
    patterns: &[Regex],
    stdout: &io::Stdout,
) {
    let chunk_uri = json_val
        .get("params")
        .and_then(|p| p.get("uri"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut diagnostics = json_val
        .get("params")
        .and_then(|p| p.get("diagnostics"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Same known-false-positive filtering as before, applied per chunk
    // before it's merged into the real document's diagnostics.
    diagnostics.retain(|d| {
        let message = d.get("message").and_then(Value::as_str).unwrap_or("");
        !patterns.iter().any(|re| re.is_match(message))
    });

    let merged = {
        let mut st = state.lock().unwrap();
        let Some((real_uri, idx)) = st.chunk_owner.get(&chunk_uri).cloned() else {
            // Diagnostics for a chunk we no longer track (superseded by a
            // newer edit, or already closed) — drop them.
            return;
        };
        let Some(doc) = st.documents.get_mut(&real_uri) else {
            return;
        };

        let start_line = doc.chunks[idx].start_line;
        for d in diagnostics.iter_mut() {
            remap_range(d, start_line);
        }
        doc.chunks[idx].diagnostics = diagnostics;

        let all: Vec<Value> = doc
            .chunks
            .iter()
            .flat_map(|c| c.diagnostics.clone())
            .collect();
        let version = doc.version;
        (real_uri, version, all)
    };

    let (real_uri, version, all) = merged;

    let notif = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": real_uri,
            "version": version,
            "diagnostics": all,
        }
    });

    let bytes = serde_json::to_vec(&notif).unwrap();
    let mut handle = stdout.lock();
    write_message(&mut handle, &bytes);
}

/// Shifts a diagnostic's range (and any relatedInformation locations) down
/// by `start_line`, converting chunk-local line numbers back into the real
/// document's line numbers.
fn remap_range(diagnostic: &mut Value, start_line: u32) {
    if let Some(range) = diagnostic.get_mut("range") {
        shift_line(range, "start", start_line);
        shift_line(range, "end", start_line);
    }
    if let Some(related) = diagnostic
        .get_mut("relatedInformation")
        .and_then(Value::as_array_mut)
    {
        for r in related.iter_mut() {
            if let Some(range) = r.get_mut("location").and_then(|l| l.get_mut("range")) {
                shift_line(range, "start", start_line);
                shift_line(range, "end", start_line);
            }
        }
    }
}

fn shift_line(range: &mut Value, key: &str, start_line: u32) {
    if let Some(pos) = range.get_mut(key) {
        if let Some(line) = pos.get("line").and_then(Value::as_u64) {
            pos["line"] = json!(line + start_line as u64);
        }
    }
}

// ---------------------------------------------------------------------
// Shared LSP framing helpers
// ---------------------------------------------------------------------

fn forward_value<W: Write>(value: &Value, writer: &SharedWriter<W>) {
    if let Ok(bytes) = serde_json::to_vec(value) {
        let mut w = writer.lock().unwrap();
        write_message(&mut *w, &bytes);
    }
}

fn forward_raw<W: Write>(body: &[u8], writer: &SharedWriter<W>) {
    let mut w = writer.lock().unwrap();
    write_message(&mut *w, body);
}

/// Reads one LSP Content-Length-framed message body from `reader`.
/// Returns None on EOF.
fn read_message<R: BufRead>(reader: &mut R) -> Option<Vec<u8>> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).ok()?;
        if bytes_read == 0 {
            return None; // EOF
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break; // blank line: end of headers
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
        // Any other headers (e.g. Content-Type) are ignored; plsqllang-server
        // doesn't send them in practice, and we don't need to preserve them.
    }

    let len = content_length?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}

/// Writes one LSP Content-Length-framed message to `writer`.
fn write_message<W: Write>(writer: &mut W, body: &[u8]) {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let _ = writer.write_all(header.as_bytes());
    let _ = writer.write_all(body);
    let _ = writer.flush();
}
