use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoCategory {
    Small,
    Medium,
    Large,
    VeryLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RepoStats {
    pub repo_name: String,
    pub repo_root: String,
    pub commit_sha: String,
    pub tracked_files: u64,
    pub searchable_files: u64,
    pub searchable_bytes: u64,
    pub total_disk_bytes: u64,
    pub skipped_binary_files: u64,
    pub skipped_hidden_files: u64,
    pub skipped_ignored_files: u64,
    pub languages: Vec<(String, u64)>,
    pub category: Option<RepoCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BuildStats {
    pub completed_at: String,
    pub docs_indexed: u64,
    pub files_skipped: u64,
    pub total_postings: u64,
    pub index_bytes: u64,
    pub build_millis: u128,
    pub update_millis: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FileFingerprint {
    pub size: u64,
    pub modified_unix_secs: i64,
    pub hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IndexMetadata {
    pub schema_version: u32,
    pub repo_stats: RepoStats,
    pub build_stats: BuildStats,
    pub delta_docs: u64,
    pub delta_removed_paths: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MachineInfo {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub architecture: String,
    pub logical_cores: usize,
    pub generated_at: String,
}

pub fn classify_repo(searchable_files: u64, searchable_bytes: u64) -> RepoCategory {
    if searchable_files > 500_000 || searchable_bytes > 20 * 1024 * 1024 * 1024 {
        RepoCategory::VeryLarge
    } else if searchable_files >= 50_000 || searchable_bytes >= 2 * 1024 * 1024 * 1024 {
        RepoCategory::Large
    } else if searchable_files >= 5_000 || searchable_bytes >= 200 * 1024 * 1024 {
        RepoCategory::Medium
    } else {
        RepoCategory::Small
    }
}

impl RepoStats {
    pub fn finalize_category(&mut self) {
        self.category = Some(classify_repo(self.searchable_files, self.searchable_bytes));
    }
}

impl BuildStats {
    pub fn completed_now() -> String {
        OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string())
    }
}
