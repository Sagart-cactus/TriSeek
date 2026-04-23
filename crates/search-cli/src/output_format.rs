//! Human-readable output formatting shared between the CLI and the MCP
//! server's prose digest.
//!
//! Two entry points:
//!
//! - `render_human` — formats a [`SearchResponse`] for CLI output, with
//!   optional ANSI colors and terminal-width-aware preview trimming.
//! - `render_digest` — formats an already-built MCP envelope [`Value`] as a
//!   plain-text digest suitable for an LLM to read directly. Never emits
//!   ANSI codes.
//!
//! Both treat results identically: group by file, show a per-file match
//! count, then indented `line:col  preview` rows, and trim previews to a
//! sensible width with a trailing `…`.
//!
//! The JSON envelope schema (`version: "1"`) is unchanged; this module only
//! touches presentation.

use search_core::{SearchEngineKind, SearchHit, SearchResponse};
use serde_json::Value;
use std::fmt::Write as _;

/// Fallback column count when we can't detect the terminal width.
const DEFAULT_CLI_COLS: usize = 120;

/// Minimum width we reserve for a match preview, regardless of terminal.
const MIN_PREVIEW_WIDTH: usize = 40;

/// ANSI escape codes. Emitted only when `RenderOpts::color` is true.
mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const CYAN: &str = "\x1b[36m";
    pub const YELLOW: &str = "\x1b[33m";
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOpts {
    pub color: bool,
    pub max_line_width: usize,
}

impl RenderOpts {
    /// Options for CLI human output. Pass the terminal width (or None for
    /// the default) and whether the stream is a color-capable TTY.
    pub fn human(terminal_cols: Option<usize>, color: bool) -> Self {
        let cols = terminal_cols.unwrap_or(DEFAULT_CLI_COLS).max(60);
        Self {
            color,
            max_line_width: cols,
        }
    }
}

/// Format a [`SearchResponse`] for CLI human output.
pub fn render_human(response: &SearchResponse, opts: RenderOpts) -> String {
    let mut out = String::new();
    let style = Style::new(opts.color);

    // Header line: "triseek  indexed · 3 files · 5 matches · 12 ms"
    let strategy = strategy_label(response.engine);
    let wall_ms = response.metrics.process.wall_millis;
    let header_body = format!(
        "{strategy} · {files} file{fp} · {matches} match{mp} · {wall:.0} ms",
        strategy = strategy,
        files = response.summary.files_with_matches,
        fp = plural(response.summary.files_with_matches),
        matches = response.summary.total_line_matches,
        mp = match_plural(response.summary.total_line_matches),
        wall = wall_ms,
    );
    writeln!(
        &mut out,
        "{} {}",
        style.bold("triseek"),
        style.dim(&header_body)
    )
    .unwrap();

    if response.hits.is_empty() {
        writeln!(&mut out).unwrap();
        writeln!(
            &mut out,
            "{}",
            style.dim(&format!(
                "no matches for \"{}\" — try --kind regex or widening --glob patterns",
                response.request.pattern
            ))
        )
        .unwrap();
        return out;
    }

    // Gutter width: the `line:col` column needs to fit the widest value we
    // expect. Compute it up front so every row aligns.
    let gutter = compute_gutter(&response.hits);
    let prefix_overhead = 2 /* indent */ + gutter + 2 /* double space */;
    let preview_width = opts
        .max_line_width
        .saturating_sub(prefix_overhead)
        .max(MIN_PREVIEW_WIDTH);

    for hit in &response.hits {
        writeln!(&mut out).unwrap();
        match hit {
            SearchHit::Content { path, lines } => {
                writeln!(
                    &mut out,
                    "{path}  {count}",
                    path = style.cyan_bold(path),
                    count = style.dim(&format!("({})", lines.len())),
                )
                .unwrap();
                for line in lines {
                    let loc = format!("{}:{}", line.line_number, line.column);
                    let preview = trim_preview(&line.line_text, preview_width);
                    writeln!(
                        &mut out,
                        "  {:>gutter$}  {}",
                        style.yellow(&loc),
                        preview,
                        gutter = gutter + style.yellow_overhead(),
                    )
                    .unwrap();
                }
            }
            SearchHit::Path { path } => {
                writeln!(&mut out, "{}", style.cyan_bold(path)).unwrap();
            }
        }
    }

    // Footer: truncation hint, only when the summary counted more than we
    // actually have room to display. This mirrors MCP's `truncated` logic.
    if is_truncated(response) {
        writeln!(&mut out).unwrap();
        writeln!(
            &mut out,
            "{}",
            style.dim(&format!(
                "showing {} of {} files — pass --max-results to widen",
                response.hits.len(),
                response.summary.files_with_matches,
            ))
        )
        .unwrap();
    }

    out
}

/// Format an MCP tool envelope as a plain-text digest. Never emits ANSI.
///
/// For search tools the envelope is produced by `build_envelope` and carries
/// `strategy`, `files_with_matches`, `total_line_matches`, `results`,
/// `truncated`, `cache`, and `fallback_used`. For non-search tools the
/// envelope shape is tool-specific and handled by a small per-tool match.
pub fn render_digest(tool_name: &str, envelope: &Value, query_hint: Option<&str>) -> String {
    match tool_name {
        "find_files" | "search_content" | "search_path_and_content" => {
            render_search_envelope_digest(tool_name, envelope, query_hint)
        }
        "index_status" => render_index_status_digest(envelope),
        "reindex" => render_reindex_digest(envelope),
        "memo_status" | "memo_session" | "memo_check" => render_memo_digest(tool_name, envelope),
        _ => envelope.to_string(),
    }
}

/// Format an MCP tool error body as a one-line prose message.
pub fn render_error_digest(tool_name: &str, error_value: &Value) -> String {
    let body = error_value.get("error").unwrap_or(&Value::Null);
    let code = body.get("code").and_then(Value::as_str).unwrap_or("ERROR");
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("tool call failed");
    let suggested = body.get("suggested_action").and_then(Value::as_str);
    match suggested {
        Some(action) => format!("{tool_name} error [{code}]: {message}. Suggested: {action}"),
        None => format!("{tool_name} error [{code}]: {message}"),
    }
}

/// Trim `text` to at most `max_chars` Unicode scalar values, appending `…`
/// when truncated.
pub fn trim_preview(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim_end_matches(['\n', '\r']);
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{head}…")
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn render_search_envelope_digest(
    tool_name: &str,
    envelope: &Value,
    query_hint: Option<&str>,
) -> String {
    let strategy = envelope
        .get("strategy")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let cache = envelope
        .get("cache")
        .and_then(Value::as_str)
        .unwrap_or("miss");
    let fallback_used = envelope
        .get("fallback_used")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let files_with_matches = envelope
        .get("files_with_matches")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_line_matches = envelope
        .get("total_line_matches")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let truncated = envelope
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let results = envelope
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let reuse_status = envelope.get("reuse_status").and_then(Value::as_str);
    let search_id = envelope
        .get("search_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let mut out = String::new();
    let query_part = match query_hint {
        Some(q) => format!(": \"{}\"", q),
        None => String::new(),
    };
    let fallback_suffix = if fallback_used { " · fallback" } else { "" };
    writeln!(
        &mut out,
        "{tool_name}{query_part} ({strategy}, cache {cache}{fallback_suffix}) — {files} file{fp}, {matches} match{mp}",
        files = files_with_matches,
        fp = plural(files_with_matches as usize),
        matches = total_line_matches,
        mp = match_plural(total_line_matches as usize),
    )
    .unwrap();

    if matches!(reuse_status, Some("fresh_duplicate")) {
        writeln!(&mut out).unwrap();
        writeln!(
            &mut out,
            "reuse prior result from context ({search_id}); no relevant files changed since the earlier search"
        )
        .unwrap();
        return out;
    }

    if results.is_empty() {
        writeln!(&mut out).unwrap();
        writeln!(
            &mut out,
            "no matches — try a different mode (regex/path) or relax any path filter"
        )
        .unwrap();
        return out;
    }

    for entry in &results {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let matches = entry.get("matches").and_then(Value::as_array);

        writeln!(&mut out).unwrap();
        match matches {
            Some(lines) if !lines.is_empty() => {
                writeln!(&mut out, "{path} ({})", lines.len()).unwrap();
                let gutter = compute_gutter_from_envelope(lines);
                for line in lines {
                    let line_no = line.get("line").and_then(Value::as_u64).unwrap_or(0);
                    let col = line.get("column").and_then(Value::as_u64).unwrap_or(0);
                    let preview = line.get("preview").and_then(Value::as_str).unwrap_or("");
                    let loc = format!("L{line_no}:{col}");
                    writeln!(
                        &mut out,
                        "  {:<gutter$}  {preview}",
                        loc,
                        gutter = gutter + 1
                    )
                    .unwrap();
                }
            }
            _ => {
                // Path-only hit (find_files or reason=path_only).
                writeln!(&mut out, "{path}").unwrap();
            }
        }
    }

    if truncated {
        writeln!(&mut out).unwrap();
        writeln!(
            &mut out,
            "[truncated: more results available — raise `limit` (max 100) or narrow the query]"
        )
        .unwrap();
    }

    out
}

fn render_index_status_digest(envelope: &Value) -> String {
    let present = envelope
        .get("index_present")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !present {
        return "index_status: no index present — call `reindex` to build one".to_string();
    }
    let files = envelope
        .get("indexed_files")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let bytes = envelope
        .get("index_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let fresh = envelope
        .get("index_fresh")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let last_updated = envelope
        .get("last_updated")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let category = envelope
        .get("repo_category")
        .and_then(Value::as_str)
        .unwrap_or("");
    let stale = if fresh { "fresh" } else { "stale" };
    let cat_part = if category.is_empty() {
        String::new()
    } else {
        format!(", {category} repo")
    };
    format!(
        "index_status: present ({stale}), {files} files, {size}{cat_part}, last built {last_updated}",
        size = format_bytes(bytes),
    )
}

fn render_reindex_digest(envelope: &Value) -> String {
    let mode = envelope.get("mode").and_then(Value::as_str).unwrap_or("?");
    let rebuilt_full = envelope
        .get("rebuilt_full")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let elapsed = envelope
        .get("elapsed_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let files = envelope
        .get("indexed_files")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let kind = if rebuilt_full {
        "full rebuild"
    } else {
        "incremental"
    };
    format!("reindex: {kind} (mode={mode}) · {files} files in {elapsed} ms")
}

fn render_memo_digest(tool_name: &str, envelope: &Value) -> String {
    let mut summary = format!("{tool_name}: ");
    if let Some(obj) = envelope.as_object() {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in obj {
            match v {
                Value::Bool(b) => parts.push(format!("{k}={b}")),
                Value::Number(n) => parts.push(format!("{k}={n}")),
                Value::String(s) if s.len() <= 80 => parts.push(format!("{k}=\"{s}\"")),
                _ => {}
            }
        }
        if parts.is_empty() {
            summary.push_str(&envelope.to_string());
        } else {
            summary.push_str(&parts.join(" · "));
        }
    } else {
        summary.push_str(&envelope.to_string());
    }
    summary
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn compute_gutter(hits: &[SearchHit]) -> usize {
    let mut max = 5; // min gutter "L1:1 " width
    for hit in hits {
        if let SearchHit::Content { lines, .. } = hit {
            for line in lines {
                let w = digit_count(line.line_number) + 1 + digit_count(line.column);
                if w > max {
                    max = w;
                }
            }
        }
    }
    max
}

fn compute_gutter_from_envelope(lines: &[Value]) -> usize {
    let mut max = 5;
    for line in lines {
        let ln = line.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
        let col = line.get("column").and_then(Value::as_u64).unwrap_or(0) as usize;
        let w = 1 /* 'L' */ + digit_count(ln) + 1 + digit_count(col);
        if w > max {
            max = w;
        }
    }
    max
}

fn digit_count(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        let mut n = n;
        let mut c = 0;
        while n > 0 {
            c += 1;
            n /= 10;
        }
        c
    }
}

fn is_truncated(response: &SearchResponse) -> bool {
    // CLI mirrors the same "summary says more than we show" check MCP uses.
    let actual_files = count_files(&response.hits);
    response.summary.files_with_matches > actual_files
}

fn count_files(hits: &[SearchHit]) -> usize {
    hits.iter()
        .map(|h| match h {
            SearchHit::Content { path, .. } | SearchHit::Path { path } => path.as_str(),
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn strategy_label(engine: SearchEngineKind) -> &'static str {
    match engine {
        SearchEngineKind::Indexed | SearchEngineKind::Auto => "indexed",
        SearchEngineKind::DirectScan => "direct scan",
        SearchEngineKind::Ripgrep => "ripgrep fallback",
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn match_plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "es" }
}

// ---------------------------------------------------------------------------
// ANSI styling — no-ops when color is disabled
// ---------------------------------------------------------------------------

struct Style {
    color: bool,
}

impl Style {
    fn new(color: bool) -> Self {
        Self { color }
    }

    fn bold(&self, s: &str) -> String {
        if self.color {
            format!("{}{}{}", ansi::BOLD, s, ansi::RESET)
        } else {
            s.to_string()
        }
    }

    fn dim(&self, s: &str) -> String {
        if self.color {
            format!("{}{}{}", ansi::DIM, s, ansi::RESET)
        } else {
            s.to_string()
        }
    }

    fn cyan_bold(&self, s: &str) -> String {
        if self.color {
            format!("{}{}{}{}", ansi::BOLD, ansi::CYAN, s, ansi::RESET)
        } else {
            s.to_string()
        }
    }

    fn yellow(&self, s: &str) -> String {
        if self.color {
            format!("{}{}{}", ansi::YELLOW, s, ansi::RESET)
        } else {
            s.to_string()
        }
    }

    /// Extra byte width introduced by ANSI wrapping of yellow text. Used so
    /// the `>gutter$` width specifier still produces visual alignment.
    fn yellow_overhead(&self) -> usize {
        if self.color {
            ansi::YELLOW.len() + ansi::RESET.len()
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use search_core::{
        AdaptiveRoute, AdaptiveRoutingDecision, CaseMode, QueryPlan, QueryRequest,
        QuerySelectivity, QueryShape, SearchEngineKind, SearchExecutionStrategy, SearchHit,
        SearchKind, SearchLineMatch, SearchResponse, SearchSummary,
    };
    use serde_json::json;

    fn sample_response(hits: Vec<SearchHit>, files: usize, matches: usize) -> SearchResponse {
        SearchResponse {
            request: QueryRequest {
                kind: SearchKind::Literal,
                engine: SearchEngineKind::Auto,
                pattern: "search".to_string(),
                case_mode: CaseMode::Sensitive,
                ..QueryRequest::default()
            },
            effective_kind: SearchKind::Literal,
            engine: SearchEngineKind::Indexed,
            routing: AdaptiveRoutingDecision {
                requested_engine: SearchEngineKind::Auto,
                selected_engine: AdaptiveRoute::Indexed,
                reason: "test".into(),
            },
            plan: QueryPlan {
                shape: QueryShape::Literal,
                selectivity: QuerySelectivity::High,
                strategy: SearchExecutionStrategy::Indexed,
                literal_seeds: Vec::new(),
                fallback_reason: None,
            },
            hits,
            summary: SearchSummary {
                files_with_matches: files,
                total_line_matches: matches,
            },
            metrics: Default::default(),
        }
    }

    #[test]
    fn trim_preview_leaves_short_text_alone() {
        assert_eq!(trim_preview("hello", 20), "hello");
    }

    #[test]
    fn trim_preview_adds_ellipsis_when_clipped() {
        let s = "abcdefghij";
        let out = trim_preview(s, 5);
        assert_eq!(out, "abcd…");
    }

    #[test]
    fn trim_preview_strips_trailing_newline() {
        assert_eq!(trim_preview("hello\n", 20), "hello");
    }

    #[test]
    fn render_human_no_matches_shows_hint() {
        let resp = sample_response(Vec::new(), 0, 0);
        let out = render_human(&resp, RenderOpts::human(Some(120), false));
        assert!(out.contains("no matches"));
        assert!(out.contains("search"));
        assert!(
            !out.contains("\x1b["),
            "should contain no ANSI when color=false"
        );
    }

    #[test]
    fn render_human_groups_by_file() {
        let hits = vec![
            SearchHit::Content {
                path: "src/lib.rs".into(),
                lines: vec![
                    SearchLineMatch {
                        line_number: 42,
                        column: 7,
                        line_text: "pub fn search(q: &str) {}".into(),
                    },
                    SearchLineMatch {
                        line_number: 89,
                        column: 11,
                        line_text: "let h = search(q);".into(),
                    },
                ],
            },
            SearchHit::Content {
                path: "src/query.rs".into(),
                lines: vec![SearchLineMatch {
                    line_number: 15,
                    column: 5,
                    line_text: "fn normalize_search_kind() {}".into(),
                }],
            },
        ];
        let resp = sample_response(hits, 2, 3);
        let out = render_human(&resp, RenderOpts::human(Some(120), false));
        assert!(out.contains("src/lib.rs"));
        assert!(out.contains("(2)"));
        assert!(out.contains("42:7"));
        assert!(out.contains("89:11"));
        assert!(out.contains("src/query.rs"));
        assert!(out.contains("(1)"));
        assert!(out.contains("15:5"));
        assert!(!out.contains("\x1b["));
    }

    #[test]
    fn render_human_with_color_emits_ansi() {
        let hits = vec![SearchHit::Content {
            path: "src/lib.rs".into(),
            lines: vec![SearchLineMatch {
                line_number: 1,
                column: 1,
                line_text: "fn search() {}".into(),
            }],
        }];
        let resp = sample_response(hits, 1, 1);
        let out = render_human(&resp, RenderOpts::human(Some(120), true));
        assert!(out.contains("\x1b[1m"));
        assert!(out.contains("\x1b[36m"));
    }

    #[test]
    fn render_human_truncation_footer_fires() {
        let hits = vec![SearchHit::Content {
            path: "a.rs".into(),
            lines: vec![SearchLineMatch {
                line_number: 1,
                column: 1,
                line_text: "x".into(),
            }],
        }];
        // Summary says 5 files but we're only rendering 1 — should trigger hint.
        let resp = sample_response(hits, 5, 10);
        let out = render_human(&resp, RenderOpts::human(Some(120), false));
        assert!(out.contains("showing 1 of 5 files"));
    }

    #[test]
    fn render_human_path_only_hits() {
        let hits = vec![
            SearchHit::Path {
                path: "docs/intro.md".into(),
            },
            SearchHit::Path {
                path: "docs/guide.md".into(),
            },
        ];
        let resp = sample_response(hits, 2, 0);
        let out = render_human(&resp, RenderOpts::human(Some(120), false));
        assert!(out.contains("docs/intro.md"));
        assert!(out.contains("docs/guide.md"));
    }

    #[test]
    fn render_search_digest_is_plain_text() {
        let envelope = json!({
            "version": "1",
            "strategy": "triseek_indexed",
            "fallback_used": false,
            "cache": "miss",
            "files_with_matches": 1,
            "total_line_matches": 1,
            "results": [{
                "path": "src/lib.rs",
                "matches": [{"line": 42, "column": 7, "preview": "pub fn search()"}],
                "reason": "content_match",
            }],
            "truncated": false,
        });
        let out = render_digest("search_content", &envelope, Some("search"));
        assert!(out.starts_with("search_content: \"search\""));
        assert!(out.contains("triseek_indexed"));
        assert!(out.contains("cache miss"));
        assert!(out.contains("src/lib.rs (1)"));
        assert!(out.contains("L42:7"));
        assert!(out.contains("pub fn search()"));
        assert!(!out.contains("\x1b["));
    }

    #[test]
    fn render_search_digest_empty() {
        let envelope = json!({
            "strategy": "triseek_indexed",
            "fallback_used": false,
            "cache": "miss",
            "files_with_matches": 0,
            "total_line_matches": 0,
            "results": [],
            "truncated": false,
        });
        let out = render_digest("search_content", &envelope, Some("zzz"));
        assert!(out.contains("no matches"));
    }

    #[test]
    fn render_search_digest_truncated_callout() {
        let envelope = json!({
            "strategy": "triseek_indexed",
            "fallback_used": false,
            "cache": "hit",
            "files_with_matches": 1,
            "total_line_matches": 1,
            "results": [{
                "path": "src/lib.rs",
                "matches": [{"line": 1, "column": 1, "preview": "x"}],
                "reason": "content_match",
            }],
            "truncated": true,
        });
        let out = render_digest("search_content", &envelope, None);
        assert!(out.contains("[truncated:"));
        assert!(out.contains("raise `limit`"));
    }

    #[test]
    fn render_search_digest_fallback_suffix() {
        let envelope = json!({
            "strategy": "ripgrep_fallback",
            "fallback_used": true,
            "cache": "bypass",
            "files_with_matches": 0,
            "total_line_matches": 0,
            "results": [],
            "truncated": false,
        });
        let out = render_digest("search_content", &envelope, None);
        assert!(out.contains("fallback"));
        assert!(out.contains("bypass"));
    }

    #[test]
    fn render_search_digest_reuse_callout() {
        let envelope = json!({
            "strategy": "triseek_indexed",
            "fallback_used": false,
            "cache": "hit",
            "reuse_status": "fresh_duplicate",
            "search_id": "search-000001",
            "files_with_matches": 3,
            "total_line_matches": 5,
            "results": [],
            "truncated": false,
        });
        let out = render_digest("search_content", &envelope, Some("AuthConfig"));
        assert!(out.contains("reuse prior result from context"));
        assert!(out.contains("search-000001"));
        assert!(!out.contains("no matches"));
    }

    #[test]
    fn render_index_status_digest_missing() {
        let envelope = json!({"index_present": false});
        let out = render_digest("index_status", &envelope, None);
        assert!(out.contains("no index present"));
    }

    #[test]
    fn render_index_status_digest_present() {
        let envelope = json!({
            "index_present": true,
            "index_fresh": true,
            "indexed_files": 1234,
            "index_bytes": 4_300_000u64,
            "last_updated": "2026-04-17T10:00:00Z",
            "repo_category": "medium",
        });
        let out = render_digest("index_status", &envelope, None);
        assert!(out.contains("1234 files"));
        assert!(out.contains("MB"));
        assert!(out.contains("medium repo"));
        assert!(out.contains("fresh"));
    }

    #[test]
    fn render_error_digest_includes_code_and_hint() {
        let err = json!({
            "version": "1",
            "error": {
                "code": "INVALID_QUERY",
                "message": "`query` must not be empty",
                "retryable": false,
                "suggested_action": "Provide a non-empty query string and a valid mode",
            },
        });
        let out = render_error_digest("search_content", &err);
        assert!(out.contains("INVALID_QUERY"));
        assert!(out.contains("must not be empty"));
        assert!(out.contains("Suggested:"));
    }
}
