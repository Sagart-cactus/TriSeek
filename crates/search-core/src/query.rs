use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    Literal,
    Regex,
    Path,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchEngineKind {
    Indexed,
    DirectScan,
    Ripgrep,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaseMode {
    #[default]
    Sensitive,
    Insensitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryRequest {
    pub kind: SearchKind,
    pub engine: SearchEngineKind,
    pub pattern: String,
    pub case_mode: CaseMode,
    pub path_substrings: Vec<String>,
    pub path_prefixes: Vec<String>,
    pub exact_paths: Vec<String>,
    pub exact_names: Vec<String>,
    pub extensions: Vec<String>,
    pub globs: Vec<String>,
    pub include_hidden: bool,
    pub include_binary: bool,
    pub max_results: Option<usize>,
}

impl Default for QueryRequest {
    fn default() -> Self {
        Self {
            kind: SearchKind::Auto,
            engine: SearchEngineKind::Auto,
            pattern: String::new(),
            case_mode: CaseMode::Sensitive,
            path_substrings: Vec::new(),
            path_prefixes: Vec::new(),
            exact_paths: Vec::new(),
            exact_names: Vec::new(),
            extensions: Vec::new(),
            globs: Vec::new(),
            include_hidden: false,
            include_binary: false,
            max_results: None,
        }
    }
}

impl QueryRequest {
    pub fn normalized_extensions(&self) -> Vec<String> {
        self.extensions
            .iter()
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .filter(|ext| !ext.is_empty())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionQuery {
    pub name: String,
    #[serde(flatten)]
    pub request: QueryRequest,
}
