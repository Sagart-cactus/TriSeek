use crate::output_format::trim_preview;
use crate::search_runner;
use anyhow::{Result, bail};
use search_core::{CaseMode, QueryRequest, SearchEngineKind, SearchHit, SearchKind};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const DEFAULT_BUDGET_TOKENS: usize = 1200;
pub const DEFAULT_MAX_FILES: usize = 4;
pub const HARD_BUDGET_TOKENS: usize = 4000;
pub const HARD_MAX_FILES: usize = 12;

const ENVELOPE_VERSION: &str = "1";
const MAX_TERMS: usize = 6;
const SEARCH_LIMIT: usize = 20;
const SNIPPET_MAX_CHARS: usize = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackIntent {
    Bugfix,
    Review,
}

impl ContextPackIntent {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("bugfix") {
            "bugfix" => Ok(Self::Bugfix),
            "review" => Ok(Self::Review),
            other => bail!("invalid context pack intent `{other}`; expected `bugfix` or `review`"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bugfix => "bugfix",
            Self::Review => "review",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextPackRequest {
    pub goal: String,
    pub intent: ContextPackIntent,
    pub budget_tokens: Option<usize>,
    pub max_files: Option<usize>,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ContextPackEnvelope {
    pub version: &'static str,
    pub repo_root: String,
    pub intent: &'static str,
    pub goal: String,
    pub budget_tokens: usize,
    pub max_files: usize,
    pub estimated_tokens: usize,
    pub items: Vec<ContextPackItem>,
    pub suggested_next_steps: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct ContextPackItem {
    pub path: String,
    pub score: f64,
    pub reasons: Vec<String>,
    pub snippets: Vec<ContextPackSnippet>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ContextPackSnippet {
    pub line: usize,
    pub column: usize,
    pub preview: String,
}

#[derive(Debug, Default)]
struct Candidate {
    score: f64,
    reasons: BTreeSet<String>,
    snippets: Vec<ContextPackSnippet>,
}

pub fn build_context_pack(
    repo_root: &Path,
    index_dir: &Path,
    request: ContextPackRequest,
) -> Result<ContextPackEnvelope> {
    let goal = request.goal.trim().to_string();
    if goal.is_empty() {
        bail!("`goal` must not be empty for context_pack");
    }

    let budget_tokens = clamp_budget(request.budget_tokens);
    let max_files = clamp_max_files(request.max_files);
    let terms = extract_terms(&goal);
    let mut candidates = BTreeMap::<String, Candidate>::new();

    for term in terms.iter().take(MAX_TERMS) {
        collect_content_candidates(repo_root, index_dir, term, &mut candidates);
        collect_path_candidates(repo_root, index_dir, term, &mut candidates);
    }

    for path in normalize_changed_files(&request.changed_files) {
        let candidate = candidates.entry(path.clone()).or_default();
        candidate.score += 8.0;
        candidate.reasons.insert("changed_file".to_string());
        if matches!(request.intent, ContextPackIntent::Review) {
            candidate.score += 4.0;
        }
    }

    apply_intent_heuristics(request.intent, &terms, &mut candidates);
    add_test_adjacency(repo_root, index_dir, &terms, &mut candidates);

    let mut ranked: Vec<(String, Candidate)> = candidates.into_iter().collect();
    ranked.sort_by(|(path_a, a), (path_b, b)| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| path_a.cmp(path_b))
    });

    let total_candidates = ranked.len();
    let mut estimated_tokens = 0_usize;
    let mut items = Vec::new();
    for (path, candidate) in ranked.into_iter() {
        if items.len() >= max_files {
            break;
        }
        let mut snippets = Vec::new();
        for snippet in candidate.snippets {
            let snippet_tokens = estimate_tokens(&snippet.preview) + 8;
            if estimated_tokens + snippet_tokens > budget_tokens && !snippets.is_empty() {
                break;
            }
            if estimated_tokens + snippet_tokens > budget_tokens && !items.is_empty() {
                break;
            }
            estimated_tokens += snippet_tokens;
            snippets.push(snippet);
        }
        let path_tokens = estimate_tokens(&path) + 12;
        if estimated_tokens + path_tokens > budget_tokens && !items.is_empty() {
            break;
        }
        estimated_tokens += path_tokens;
        items.push(ContextPackItem {
            path,
            score: round_score(candidate.score),
            reasons: candidate.reasons.into_iter().collect(),
            snippets,
        });
    }

    let truncated = total_candidates > items.len();
    Ok(ContextPackEnvelope {
        version: ENVELOPE_VERSION,
        repo_root: repo_root.display().to_string(),
        intent: request.intent.as_str(),
        goal,
        budget_tokens,
        max_files,
        estimated_tokens,
        items,
        suggested_next_steps: suggested_next_steps(request.intent, &terms),
        truncated,
    })
}

pub fn clamp_budget(value: Option<usize>) -> usize {
    value
        .unwrap_or(DEFAULT_BUDGET_TOKENS)
        .clamp(1, HARD_BUDGET_TOKENS)
}

pub fn clamp_max_files(value: Option<usize>) -> usize {
    value.unwrap_or(DEFAULT_MAX_FILES).clamp(1, HARD_MAX_FILES)
}

fn collect_content_candidates(
    repo_root: &Path,
    index_dir: &Path,
    term: &str,
    candidates: &mut BTreeMap<String, Candidate>,
) {
    let request = QueryRequest {
        kind: SearchKind::Literal,
        engine: SearchEngineKind::Auto,
        pattern: term.to_string(),
        case_mode: CaseMode::Insensitive,
        max_results: Some(SEARCH_LIMIT),
        ..QueryRequest::default()
    };
    let Ok(executed) = search_runner::execute_search(repo_root, index_dir, &request, true, false)
    else {
        return;
    };
    for hit in executed.response.hits {
        match hit {
            SearchHit::Content { path, lines } => {
                let candidate = candidates.entry(path).or_default();
                candidate.score += 4.0;
                if is_bugfix_term(term) {
                    candidate.score += 8.0;
                }
                candidate.reasons.insert("content_match".to_string());
                for line in lines.into_iter().take(2) {
                    if candidate.snippets.len() >= 3 {
                        break;
                    }
                    candidate.snippets.push(ContextPackSnippet {
                        line: line.line_number,
                        column: line.column,
                        preview: trim_preview(&line.line_text, SNIPPET_MAX_CHARS),
                    });
                }
            }
            SearchHit::Path { path } => {
                let candidate = candidates.entry(path).or_default();
                candidate.score += 1.0;
                candidate.reasons.insert("path_match".to_string());
            }
        }
    }
}

fn collect_path_candidates(
    repo_root: &Path,
    index_dir: &Path,
    term: &str,
    candidates: &mut BTreeMap<String, Candidate>,
) {
    let request = QueryRequest {
        kind: SearchKind::Path,
        engine: SearchEngineKind::Auto,
        pattern: term.to_string(),
        case_mode: CaseMode::Insensitive,
        max_results: Some(SEARCH_LIMIT),
        ..QueryRequest::default()
    };
    let Ok(executed) = search_runner::execute_search(repo_root, index_dir, &request, true, false)
    else {
        return;
    };
    for hit in executed.response.hits {
        let path = match hit {
            SearchHit::Content { path, .. } | SearchHit::Path { path } => path,
        };
        let candidate = candidates.entry(path).or_default();
        candidate.score += 2.0;
        candidate.reasons.insert("path_match".to_string());
    }
}

fn apply_intent_heuristics(
    intent: ContextPackIntent,
    terms: &[String],
    candidates: &mut BTreeMap<String, Candidate>,
) {
    for (path, candidate) in candidates.iter_mut() {
        let lower = path.to_ascii_lowercase();
        if looks_like_test(&lower) {
            candidate.score += 3.0;
            candidate.reasons.insert("test_adjacent".to_string());
        }
        if looks_like_config(&lower) {
            candidate.score += 2.0;
            candidate.reasons.insert("config_like".to_string());
        }
        if looks_like_fixture(&lower) {
            candidate.score += 2.0;
            candidate.reasons.insert("fixture_like".to_string());
        }
        match intent {
            ContextPackIntent::Bugfix => {
                if terms.iter().any(|term| is_bugfix_term(term)) {
                    candidate.score += 2.0;
                }
                if !looks_like_test(&lower) {
                    candidate.score += 8.0;
                }
                if lower.contains("panic") || lower.contains("error") || lower.contains("fail") {
                    candidate.score += 2.0;
                }
            }
            ContextPackIntent::Review => {
                if looks_like_test(&lower) {
                    candidate.score += 2.0;
                }
            }
        }
    }
}

fn add_test_adjacency(
    repo_root: &Path,
    index_dir: &Path,
    terms: &[String],
    candidates: &mut BTreeMap<String, Candidate>,
) {
    let terms: Vec<&String> = terms
        .iter()
        .filter(|term| term.len() >= 4)
        .take(MAX_TERMS)
        .collect();
    for term in terms {
        let request = QueryRequest {
            kind: SearchKind::Path,
            engine: SearchEngineKind::Auto,
            pattern: term.to_string(),
            case_mode: CaseMode::Insensitive,
            max_results: Some(SEARCH_LIMIT),
            ..QueryRequest::default()
        };
        let Ok(executed) =
            search_runner::execute_search(repo_root, index_dir, &request, true, false)
        else {
            continue;
        };
        for hit in executed.response.hits {
            let path = match hit {
                SearchHit::Content { path, .. } | SearchHit::Path { path } => path,
            };
            if !looks_like_test(&path.to_ascii_lowercase()) {
                continue;
            }
            let candidate = candidates.entry(path.clone()).or_default();
            candidate.score += 4.0;
            candidate.reasons.insert("test_adjacent".to_string());
            if candidate.snippets.is_empty() {
                add_first_line_snippet(repo_root, &path, candidate);
            }
        }
    }
}

fn add_first_line_snippet(repo_root: &Path, path: &str, candidate: &mut Candidate) {
    let full_path = repo_root.join(path);
    let Ok(contents) = std::fs::read_to_string(full_path) else {
        return;
    };
    if let Some((idx, line)) = contents
        .lines()
        .enumerate()
        .find(|(_, line)| !line.trim().is_empty())
    {
        candidate.snippets.push(ContextPackSnippet {
            line: idx + 1,
            column: 1,
            preview: trim_preview(line, SNIPPET_MAX_CHARS),
        });
    }
}

fn normalize_changed_files(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.trim().trim_start_matches("./").to_string())
        .filter(|path| !path.is_empty())
        .collect()
}

pub fn extract_terms(goal: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut terms = Vec::new();
    for raw in goal.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let term = raw.trim().to_ascii_lowercase();
        if term.len() < 3 || STOP_WORDS.contains(&term.as_str()) {
            continue;
        }
        if seen.insert(term.clone()) {
            terms.push(term);
        }
    }
    terms
}

fn suggested_next_steps(intent: ContextPackIntent, terms: &[String]) -> Vec<String> {
    let query = if terms.is_empty() {
        "the failing symbol".to_string()
    } else {
        terms.iter().take(3).cloned().collect::<Vec<_>>().join(" ")
    };
    match intent {
        ContextPackIntent::Bugfix => vec![
            "Start with the highest-ranked source file and its snippets.".to_string(),
            "If the first file is wrong, run search_content with the strongest error or symbol term.".to_string(),
            format!("If tests are missing, search_path_and_content for `{query}` under tests."),
        ],
        ContextPackIntent::Review => vec![
            "Start with changed_file items, then inspect adjacent tests.".to_string(),
            "Use search_content for touched symbols that are not explained by the pack.".to_string(),
            "Ask for a narrower context pack if this review spans unrelated areas.".to_string(),
        ],
    }
}

fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4).max(1)
}

fn round_score(score: f64) -> f64 {
    (score * 10.0).round() / 10.0
}

fn looks_like_test(path: &str) -> bool {
    path.contains("/test")
        || path.contains("tests/")
        || path.contains("_test.")
        || path.contains(".test.")
        || path.contains("_spec.")
        || path.contains(".spec.")
}

fn looks_like_config(path: &str) -> bool {
    path.contains("config")
        || path.ends_with(".toml")
        || path.ends_with(".yaml")
        || path.ends_with(".yml")
}

fn looks_like_fixture(path: &str) -> bool {
    path.contains("fixture") || path.contains("fixtures/") || path.contains("testdata")
}

fn is_bugfix_term(term: &str) -> bool {
    matches!(
        term,
        "bug" | "bugfix" | "fix" | "panic" | "error" | "fail" | "failing" | "crash"
    )
}

const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "this", "that", "what", "where", "when", "why",
    "how", "can", "our", "your", "their", "fix", "add", "make", "use", "using",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_distinct_goal_terms_without_common_words() {
        assert_eq!(
            extract_terms("Fix the auth panic for auth service accounts"),
            vec!["auth", "panic", "service", "accounts"]
        );
    }

    #[test]
    fn clamps_pack_limits() {
        assert_eq!(clamp_budget(Some(99_999)), HARD_BUDGET_TOKENS);
        assert_eq!(clamp_max_files(Some(99)), HARD_MAX_FILES);
    }
}
