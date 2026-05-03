use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use search_core::PORTABILITY_SCHEMA_VERSION;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tar::{Archive, Builder};

pub fn export(snapshot_dir: &Path, output_path: &Path) -> Result<()> {
    let checksums = checksums_for(snapshot_dir)?;
    let tmp_checksums = snapshot_dir.join("checksums.txt");
    fs::write(&tmp_checksums, render_checksums(&checksums))?;
    let output = fs::File::create(output_path)
        .with_context(|| format!("create {}", output_path.display()))?;
    let encoder = GzEncoder::new(output, Compression::default());
    let mut builder = Builder::new(encoder);
    for path in files_under(snapshot_dir)? {
        let rel = path.strip_prefix(snapshot_dir)?;
        builder.append_path_with_name(&path, rel)?;
    }
    builder.finish()?;
    let _ = fs::remove_file(tmp_checksums);
    Ok(())
}

pub fn import(pack_path: &Path, snapshots_root: &Path) -> Result<String> {
    let tmp = snapshots_root.join(format!("import-{}", now_millis()));
    fs::create_dir_all(&tmp)?;
    let file =
        fs::File::open(pack_path).with_context(|| format!("open {}", pack_path.display()))?;
    let decoder = GzDecoder::new(file);
    Archive::new(decoder).unpack(&tmp)?;
    validate_import(&tmp)?;
    let manifest: Value = serde_json::from_slice(&fs::read(tmp.join("manifest.json"))?)?;
    let snapshot_id = manifest
        .get("snapshot_id")
        .and_then(Value::as_str)
        .context("manifest missing snapshot_id")?
        .to_string();
    let mut final_id = snapshot_id.clone();
    let mut final_dir = snapshots_root.join(&final_id);
    let mut suffix = 1;
    while final_dir.exists() {
        final_id = format!("{snapshot_id}-{suffix}");
        final_dir = snapshots_root.join(&final_id);
        suffix += 1;
    }
    if final_id != snapshot_id {
        let mut manifest = manifest;
        manifest["snapshot_id"] = Value::String(final_id.clone());
        fs::write(
            tmp.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
    }
    fs::create_dir_all(snapshots_root)?;
    fs::rename(&tmp, &final_dir)?;
    Ok(final_id)
}

fn validate_import(dir: &Path) -> Result<()> {
    let manifest: Value = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)?;
    let version = manifest
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32;
    if version != PORTABILITY_SCHEMA_VERSION {
        bail!("unsupported snapshot schema version {version}");
    }
    let expected = parse_checksums(&fs::read_to_string(dir.join("checksums.txt"))?);
    let actual = checksums_for(dir)?;
    for (path, sha) in expected {
        if path == "checksums.txt" {
            continue;
        }
        if actual.get(&path) != Some(&sha) {
            bail!("checksum mismatch for {path}");
        }
    }
    Ok(())
}

fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            out.extend(files_under(&path)?);
        } else {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn checksums_for(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut checksums = BTreeMap::new();
    for path in files_under(root)? {
        let rel = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "checksums.txt" {
            continue;
        }
        let bytes = fs::read(&path)?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        checksums.insert(rel, format!("{:x}", hasher.finalize()));
    }
    Ok(checksums)
}

fn render_checksums(checksums: &BTreeMap<String, String>) -> String {
    checksums
        .iter()
        .map(|(path, sha)| format!("{sha}  {path}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn parse_checksums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (sha, path) = line.split_once("  ")?;
            Some((path.to_string(), sha.to_string()))
        })
        .collect()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
