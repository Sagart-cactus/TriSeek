mod build;
mod engine;
mod error;
mod model;
mod storage;
mod walker;

pub use build::{BuildConfig, UpdateOutcome, build_index, measure_repository, update_index};
pub use engine::SearchEngine;
pub use error::SearchIndexError;
pub use model::{
    DeltaSnapshot, DocumentRecord, NamePostingEntry, PersistedIndex, PostingListEntry,
    RuntimeIndex, SCHEMA_VERSION, SearchExecution,
};
pub use storage::{default_index_dir, index_exists, read_index_metadata};
pub use walker::{ScanOptions, ScanSummary, ScannedFile, scan_repository, walk_repository};
