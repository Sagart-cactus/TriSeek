use anyhow::{Context, Result, bail};
use search_core::PORTABILITY_SCHEMA_VERSION;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const HANDOFF_METADATA_FILE: &str = "handoff.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffMode {
    Metadata,
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffMetadata {
    pub schema_version: u32,
    pub mode: HandoffMode,
    pub target_harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitHandoffMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHandoffMetadata {
    pub remote: String,
    pub remote_url: String,
    pub branch: String,
    pub commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    pub dirty_commit_created: bool,
    pub pushed_at: i64,
}

impl HandoffMetadata {
    pub fn metadata(target_harness: &str) -> Self {
        Self {
            schema_version: PORTABILITY_SCHEMA_VERSION,
            mode: HandoffMode::Metadata,
            target_harness: target_harness.to_string(),
            git: None,
        }
    }

    pub fn git(target_harness: &str, git: GitHandoffMetadata) -> Self {
        Self {
            schema_version: PORTABILITY_SCHEMA_VERSION,
            mode: HandoffMode::Git,
            target_harness: target_harness.to_string(),
            git: Some(git),
        }
    }
}

pub fn write(snapshot_dir: &Path, metadata: &HandoffMetadata) -> Result<()> {
    validate(metadata)?;
    fs::write(
        snapshot_dir.join(HANDOFF_METADATA_FILE),
        serde_json::to_vec_pretty(metadata)?,
    )
    .with_context(|| {
        format!(
            "write {}",
            snapshot_dir.join(HANDOFF_METADATA_FILE).display()
        )
    })
}

pub fn read(snapshot_dir: &Path) -> Result<HandoffMetadata> {
    let path = snapshot_dir.join(HANDOFF_METADATA_FILE);
    let metadata: HandoffMetadata = serde_json::from_slice(
        &fs::read(&path).with_context(|| format!("read {}", path.display()))?,
    )?;
    validate(&metadata)?;
    Ok(metadata)
}

pub fn read_optional(snapshot_dir: &Path) -> Result<Option<HandoffMetadata>> {
    let path = snapshot_dir.join(HANDOFF_METADATA_FILE);
    if !path.exists() {
        return Ok(None);
    }
    read(snapshot_dir).map(Some)
}

fn validate(metadata: &HandoffMetadata) -> Result<()> {
    if metadata.schema_version != PORTABILITY_SCHEMA_VERSION {
        bail!(
            "unsupported handoff metadata schema version {}",
            metadata.schema_version
        );
    }
    if metadata.target_harness.trim().is_empty() {
        bail!("handoff metadata missing target_harness");
    }
    match metadata.mode {
        HandoffMode::Metadata => {
            if metadata.git.is_some() {
                bail!("metadata handoff must not include git metadata");
            }
        }
        HandoffMode::Git => {
            let git = metadata
                .git
                .as_ref()
                .context("git handoff missing git metadata")?;
            if git.remote.trim().is_empty()
                || git.remote_url.trim().is_empty()
                || git.branch.trim().is_empty()
                || git.commit.trim().is_empty()
            {
                bail!("git handoff metadata missing restore fields");
            }
        }
    }
    Ok(())
}
