use crate::error::SearchIndexError;
use crate::model::{DeltaSnapshot, PersistedIndex};
use search_core::IndexMetadata;
use std::fs;
use std::path::{Path, PathBuf};

const BASE_FILE: &str = "base.bin";
const DELTA_FILE: &str = "delta.bin";
const METADATA_FILE: &str = "metadata.json";
const FAST_INDEX_FILE: &str = "fast.idx";

pub fn fast_index_path(index_dir: &Path) -> PathBuf {
    index_dir.join(FAST_INDEX_FILE)
}

pub fn fast_index_exists(index_dir: &Path) -> bool {
    index_dir.join(FAST_INDEX_FILE).exists()
}

pub fn default_index_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".triseek-index")
}

pub fn index_exists(index_dir: &Path) -> bool {
    index_dir.join(BASE_FILE).exists()
}

pub fn read_index_metadata(index_dir: &Path) -> Result<IndexMetadata, SearchIndexError> {
    let path = index_dir.join(METADATA_FILE);
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn load_base(index_dir: &Path) -> Result<PersistedIndex, SearchIndexError> {
    let path = index_dir.join(BASE_FILE);
    if !path.exists() {
        return Err(SearchIndexError::MissingIndex(path));
    }
    let bytes = fs::read(path)?;
    let (index, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard())?;
    Ok(index)
}

pub fn load_delta(index_dir: &Path) -> Result<Option<DeltaSnapshot>, SearchIndexError> {
    let path = index_dir.join(DELTA_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    let (index, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard())?;
    Ok(Some(index))
}

pub fn persist_base(index_dir: &Path, index: &PersistedIndex) -> Result<u64, SearchIndexError> {
    fs::create_dir_all(index_dir)?;
    let path = index_dir.join(BASE_FILE);
    let bytes = bincode::serde::encode_to_vec(index, bincode::config::standard())?;
    let size = bytes.len() as u64;
    fs::write(path, bytes)?;
    Ok(size)
}

pub fn persist_delta(index_dir: &Path, index: &DeltaSnapshot) -> Result<u64, SearchIndexError> {
    fs::create_dir_all(index_dir)?;
    let path = index_dir.join(DELTA_FILE);
    let bytes = bincode::serde::encode_to_vec(index, bincode::config::standard())?;
    let size = bytes.len() as u64;
    fs::write(path, bytes)?;
    Ok(size)
}

pub fn remove_delta(index_dir: &Path) -> Result<(), SearchIndexError> {
    let path = index_dir.join(DELTA_FILE);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn persist_metadata(
    index_dir: &Path,
    metadata: &IndexMetadata,
) -> Result<(), SearchIndexError> {
    fs::create_dir_all(index_dir)?;
    let path = index_dir.join(METADATA_FILE);
    let bytes = serde_json::to_vec_pretty(metadata)?;
    fs::write(path, bytes)?;
    Ok(())
}
