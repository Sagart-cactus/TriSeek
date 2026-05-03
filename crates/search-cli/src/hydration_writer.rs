use anyhow::{Context, Result};
use std::path::PathBuf;

pub enum Target {
    ProjectFile { path: PathBuf },
    Stdout,
}

pub fn write_payload(target: Target, payload: &str) -> Result<()> {
    match target {
        Target::Stdout => {
            println!("{payload}");
            Ok(())
        }
        Target::ProjectFile { path } => std::fs::write(&path, payload)
            .with_context(|| format!("write hydration payload to {}", path.display())),
    }
}
