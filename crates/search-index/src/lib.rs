mod build;
mod engine;
mod error;
pub mod fastindex;
mod model;
mod storage;
mod walker;
pub mod watcher;

pub use build::{
    BuildConfig, BuildPhase, BuildProgress, BuildProgressSnapshot, UpdateOutcome,
    apply_incremental_changes, build_index, build_index_with_progress, measure_repository,
    update_index,
};
pub use engine::SearchEngine;
pub use error::SearchIndexError;
pub use model::{
    DeltaSnapshot, DocumentRecord, NamePostingEntry, PersistedIndex, PostingListEntry,
    RuntimeIndex, SCHEMA_VERSION, SearchExecution,
};
pub use storage::{
    daemon_dir, default_index_dir, index_exists, read_index_metadata, triseek_home_dir,
};
pub use walker::{
    DEFAULT_SEARCHABLE_HIDDEN_DIRS, DEFAULT_SEARCHABLE_HIDDEN_FILES, ScanOptions, ScanSummary,
    ScannedFile, default_searchable_hidden_roots, scan_repository, walk_repository,
};
pub use watcher::{GenerationCounter, WatcherHandle, start_watcher};
