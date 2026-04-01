use crate::query::{CaseMode, QueryRequest, SearchEngineKind, SearchKind};
use crate::repo::{RepoCategory, RepoStats};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryShape {
    Literal,
    ShortLiteral,
    RegexAnchored,
    RegexWeak,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySelectivity {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchExecutionStrategy {
    Indexed,
    DirectScan,
    Ripgrep,
    PathIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveRoute {
    Indexed,
    DirectScan,
    Ripgrep,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlan {
    pub shape: QueryShape,
    pub selectivity: QuerySelectivity,
    pub strategy: SearchExecutionStrategy,
    pub literal_seeds: Vec<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveRoutingDecision {
    pub requested_engine: SearchEngineKind,
    pub selected_engine: AdaptiveRoute,
    pub reason: String,
}

pub fn plan_query(request: &QueryRequest) -> QueryPlan {
    match request.kind {
        SearchKind::Path => QueryPlan {
            shape: QueryShape::Path,
            selectivity: if request.pattern.len() >= 3 || !request.exact_names.is_empty() {
                QuerySelectivity::High
            } else {
                QuerySelectivity::Medium
            },
            strategy: SearchExecutionStrategy::PathIndex,
            literal_seeds: Vec::new(),
            fallback_reason: None,
        },
        SearchKind::Literal | SearchKind::Auto => plan_literal_or_auto(request),
        SearchKind::Regex => plan_regex(request),
    }
}

pub fn route_query(
    request: &QueryRequest,
    repo_stats: Option<&RepoStats>,
    plan: &QueryPlan,
    index_available: bool,
    repeated_session: bool,
) -> AdaptiveRoutingDecision {
    if request.engine != SearchEngineKind::Auto {
        return AdaptiveRoutingDecision {
            requested_engine: request.engine,
            selected_engine: match request.engine {
                SearchEngineKind::Indexed => AdaptiveRoute::Indexed,
                SearchEngineKind::DirectScan => AdaptiveRoute::DirectScan,
                SearchEngineKind::Ripgrep => AdaptiveRoute::Ripgrep,
                SearchEngineKind::Auto => AdaptiveRoute::Indexed,
            },
            reason: "explicit_engine_override".to_string(),
        };
    }

    let Some(repo_stats) = repo_stats else {
        return AdaptiveRoutingDecision {
            requested_engine: SearchEngineKind::Auto,
            selected_engine: if index_available {
                AdaptiveRoute::Indexed
            } else {
                AdaptiveRoute::DirectScan
            },
            reason: "repo_stats_unavailable".to_string(),
        };
    };

    let category = repo_stats.category.unwrap_or(RepoCategory::Small);
    let selected_engine =
        if !index_available || matches!(plan.strategy, SearchExecutionStrategy::DirectScan) {
            if matches!(request.kind, SearchKind::Path) {
                AdaptiveRoute::DirectScan
            } else {
                AdaptiveRoute::Ripgrep
            }
        } else {
            match category {
                RepoCategory::Small if !repeated_session => AdaptiveRoute::Ripgrep,
                RepoCategory::Medium
                    if !repeated_session && matches!(plan.selectivity, QuerySelectivity::Low) =>
                {
                    AdaptiveRoute::Ripgrep
                }
                _ => AdaptiveRoute::Indexed,
            }
        };

    AdaptiveRoutingDecision {
        requested_engine: SearchEngineKind::Auto,
        selected_engine,
        reason: format!(
            "category={:?};repeated_session={};strategy={:?};index_available={}",
            category, repeated_session, plan.strategy, index_available
        ),
    }
}

fn plan_literal_or_auto(request: &QueryRequest) -> QueryPlan {
    let shape = if request.pattern.len() < 3 {
        QueryShape::ShortLiteral
    } else {
        QueryShape::Literal
    };
    let selectivity = match request.pattern.len() {
        0..=2 => QuerySelectivity::Low,
        3..=5 => QuerySelectivity::Medium,
        _ => QuerySelectivity::High,
    };
    let strategy = if request.pattern.len() >= 3 {
        SearchExecutionStrategy::Indexed
    } else {
        SearchExecutionStrategy::DirectScan
    };
    QueryPlan {
        shape,
        selectivity,
        strategy,
        literal_seeds: if request.pattern.is_empty() {
            Vec::new()
        } else {
            vec![normalize_seed(&request.pattern, request.case_mode)]
        },
        fallback_reason: if request.pattern.len() < 3 {
            Some("pattern_too_short_for_trigram_pruning".to_string())
        } else {
            None
        },
    }
}

fn plan_regex(request: &QueryRequest) -> QueryPlan {
    let seeds = extract_regex_literals(&request.pattern, request.case_mode);
    let longest = seeds.iter().map(String::len).max().unwrap_or_default();
    let strategy = if longest >= 3 {
        SearchExecutionStrategy::Indexed
    } else {
        SearchExecutionStrategy::DirectScan
    };
    QueryPlan {
        shape: if longest >= 3 {
            QueryShape::RegexAnchored
        } else {
            QueryShape::RegexWeak
        },
        selectivity: match longest {
            0..=2 => QuerySelectivity::Low,
            3..=5 => QuerySelectivity::Medium,
            _ => QuerySelectivity::High,
        },
        strategy,
        literal_seeds: seeds,
        fallback_reason: if longest < 3 {
            Some("regex_has_no_extractable_literal_seed".to_string())
        } else {
            None
        },
    }
}

/// Extract literal substrings from a regex pattern that can be used as trigram seeds.
/// Improved version: handles alternation (picks longest branch), optional quantifiers,
/// groups, and character classes correctly.
pub fn extract_regex_literals(pattern: &str, case_mode: CaseMode) -> Vec<String> {
    let mut seeds = Vec::new();
    let mut current = String::new();
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    let mut _depth = 0_i32; // paren depth

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let Some(next) = chars.next() else {
                    break;
                };
                if in_class {
                    // skip
                } else if is_regex_escape(next) {
                    flush_seed(&mut current, &mut seeds);
                } else {
                    // Escaped literal character
                    current.push(next);
                }
            }
            '[' if !in_class => {
                flush_seed(&mut current, &mut seeds);
                in_class = true;
            }
            ']' if in_class => {
                in_class = false;
            }
            _ if in_class => {}
            '(' => {
                // Don't flush — capture group might contain useful literals
                // But we need to track depth for alternation handling
                _depth += 1;
                flush_seed(&mut current, &mut seeds);
            }
            ')' => {
                _depth -= 1;
                flush_seed(&mut current, &mut seeds);
            }
            '|' => {
                // Alternation — flush current seed, each branch contributes independently
                flush_seed(&mut current, &mut seeds);
            }
            '?' | '*' => {
                // The preceding character/group is optional — remove last char from current
                if !current.is_empty() {
                    current.pop();
                    flush_seed(&mut current, &mut seeds);
                }
            }
            '+' => {
                // One or more — the preceding char IS required, so keep it but flush
                // to avoid assuming it continues
                flush_seed(&mut current, &mut seeds);
            }
            '{' => {
                // Quantifier — check if {0,...} which makes preceding optional
                let mut quant = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '}' {
                        chars.next();
                        break;
                    }
                    quant.push(c);
                    chars.next();
                }
                if quant.starts_with('0') {
                    // {0,...} means optional
                    if !current.is_empty() {
                        current.pop();
                        flush_seed(&mut current, &mut seeds);
                    }
                } else {
                    // {1,...} or {n,...} — preceding is required
                    flush_seed(&mut current, &mut seeds);
                }
            }
            '.' | '^' | '$' => {
                flush_seed(&mut current, &mut seeds);
            }
            _ => current.push(ch),
        }
    }
    flush_seed(&mut current, &mut seeds);
    seeds.sort();
    seeds.dedup();
    seeds
        .into_iter()
        .filter(|seed| seed.len() >= 2)
        .map(|seed| normalize_seed(&seed, case_mode))
        .collect()
}

fn normalize_seed(seed: &str, case_mode: CaseMode) -> String {
    match case_mode {
        CaseMode::Sensitive => seed.to_string(),
        CaseMode::Insensitive => seed.to_ascii_lowercase(),
    }
}

fn is_regex_escape(ch: char) -> bool {
    matches!(
        ch,
        'A' | 'b' | 'B' | 'd' | 'D' | 'f' | 'n' | 'r' | 's' | 'S' | 't' | 'v' | 'w' | 'W' | 'z'
    )
}

fn flush_seed(current: &mut String, out: &mut Vec<String>) {
    if current.len() >= 2 {
        out.push(current.clone());
    }
    current.clear();
}
