use crate::error::SearchIndexError;
use crate::model::{DeltaSnapshot, PersistedIndex};
use search_core::IndexMetadata;
use std::fs;
use std::path::{Component, Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

const BASE_FILE: &str = "base.bin";
const DELTA_FILE: &str = "delta.bin";
const METADATA_FILE: &str = "metadata.json";
const FAST_INDEX_FILE: &str = "fast.idx";
const DEFAULT_HOME_DIR_NAME: &str = ".triseek";

pub fn fast_index_path(index_dir: &Path) -> PathBuf {
    index_dir.join(FAST_INDEX_FILE)
}

pub fn fast_index_exists(index_dir: &Path) -> bool {
    index_dir.join(FAST_INDEX_FILE).exists()
}

pub fn triseek_home_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("TRISEEK_HOME") {
        return absolutize_path(Path::new(&path));
    }
    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(path).join("TriSeek");
        }
        if let Some(path) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(path).join(DEFAULT_HOME_DIR_NAME);
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(path) = std::env::var_os("HOME") {
            return PathBuf::from(path).join(DEFAULT_HOME_DIR_NAME);
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(DEFAULT_HOME_DIR_NAME)
}

pub fn daemon_dir() -> PathBuf {
    triseek_home_dir().join("daemon")
}

pub fn default_index_dir(repo_root: &Path) -> PathBuf {
    triseek_home_dir()
        .join("indexes")
        .join(index_dir_key(repo_root))
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

fn index_dir_key(repo_root: &Path) -> String {
    let normalized_root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| normalize_key_path(repo_root));
    let display = normalized_root.to_string_lossy();
    #[cfg(windows)]
    let hash_input = display.to_ascii_lowercase();
    #[cfg(not(windows))]
    let hash_input = display.into_owned();
    let hash = xxh3_64(hash_input.as_bytes());
    let name = normalized_root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("root");
    format!("{}-{hash:016x}", slug_component(name))
}

fn slug_component(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    let mut last_was_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else {
            None
        };
        match normalized {
            Some(ch) => {
                slug.push(ch);
                last_was_dash = false;
            }
            None if !last_was_dash && !slug.is_empty() => {
                slug.push('-');
                last_was_dash = true;
            }
            None => {}
        }
    }
    slug.trim_matches('-')
        .to_string()
        .chars()
        .collect::<String>()
}

fn normalize_key_path(path: &Path) -> PathBuf {
    let absolute = absolutize_path(path);
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
            Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn absolutize_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn default_index_dir_uses_triseek_home_override() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let previous_home = std::env::var_os("TRISEEK_HOME");
        unsafe {
            std::env::set_var("TRISEEK_HOME", temp.path());
        }
        let repo = temp.path().join("repos/project");
        let dir = default_index_dir(&repo);
        assert!(dir.starts_with(temp.path().join("indexes")));
        if let Some(previous_home) = previous_home {
            unsafe {
                std::env::set_var("TRISEEK_HOME", previous_home);
            }
        } else {
            unsafe {
                std::env::remove_var("TRISEEK_HOME");
            }
        }
    }

    #[test]
    fn default_index_dir_is_stable_for_relative_forms() {
        let _guard = env_lock().lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let nested = temp.path().join("repos/project");
        fs::create_dir_all(&nested).unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        let previous_home = std::env::var_os("TRISEEK_HOME");
        unsafe {
            std::env::set_var("TRISEEK_HOME", &home);
        }
        std::env::set_current_dir(temp.path()).unwrap();
        let via_dot = default_index_dir(Path::new("./repos/project"));
        let via_dotdot = default_index_dir(Path::new("repos/child/../project"));
        let via_abs = default_index_dir(&nested);
        assert_eq!(via_dot, via_dotdot);
        assert_eq!(via_dot, via_abs);
        std::env::set_current_dir(original_cwd).unwrap();
        if let Some(previous_home) = previous_home {
            unsafe {
                std::env::set_var("TRISEEK_HOME", previous_home);
            }
        } else {
            unsafe {
                std::env::remove_var("TRISEEK_HOME");
            }
        }
    }
}
