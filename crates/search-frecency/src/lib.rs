use search_core::SearchHit;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

const MAX_QUERY_HISTORY: usize = 500;
const HALF_LIFE_SECS: f64 = 604_800.0; // 7 days
const RESULT_WEIGHT: f64 = 1.0;
const SELECT_WEIGHT: f64 = 3.0;

/// Per-file frecency record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrecencyEntry {
    /// Decayed cumulative score.
    pub score: f64,
    /// Unix timestamp (seconds) of last touch.
    pub last_access_secs: i64,
    /// Times this file appeared in returned results.
    pub result_count: u32,
    /// Times this file was explicitly selected/opened.
    pub select_count: u32,
}

/// A logged query event for future pattern-based learning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEvent {
    pub timestamp_secs: i64,
    pub pattern: String,
    /// "literal" | "regex" | "path"
    pub kind: String,
    /// Paths of the top results returned.
    pub result_paths: Vec<String>,
    /// Paths explicitly opened by the caller.
    pub selected_paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FrecencyData {
    version: u32,
    half_life_secs: f64,
    entries: HashMap<String, FrecencyEntry>,
    recent_queries: VecDeque<QueryEvent>,
}

impl Default for FrecencyData {
    fn default() -> Self {
        Self {
            version: 1,
            half_life_secs: HALF_LIFE_SECS,
            entries: HashMap::new(),
            recent_queries: VecDeque::new(),
        }
    }
}

/// Persistent frecency store backed by `<index_dir>/frecency.json`.
pub struct FrecencyStore {
    path: PathBuf,
    data: FrecencyData,
}

impl FrecencyStore {
    /// Load from `<index_dir>/frecency.json`. Returns an empty store on missing/corrupt file.
    pub fn open(index_dir: &Path) -> Self {
        let path = index_dir.join("frecency.json");
        let data = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        FrecencyStore { path, data }
    }

    /// True when no frecency data has been collected yet.
    pub fn is_empty(&self) -> bool {
        self.data.entries.is_empty()
    }

    /// Current decayed score for `path` (0.0 if unknown).
    pub fn score_for(&self, path: &str) -> f64 {
        if let Some(entry) = self.data.entries.get(path) {
            let dt = (now_secs() - entry.last_access_secs).max(0) as f64;
            entry.score * decay(dt, self.data.half_life_secs)
        } else {
            0.0
        }
    }

    /// Record that these hits appeared in search results (weight = 1.0).
    pub fn record_results(&mut self, hits: &[SearchHit]) {
        let now = now_secs();
        for hit in hits {
            self.touch(hit_path(hit), RESULT_WEIGHT, now);
            if let Some(entry) = self.data.entries.get_mut(hit_path(hit)) {
                entry.result_count += 1;
            }
        }
    }

    /// Record that these paths were explicitly opened/used (weight = 3.0).
    pub fn record_select(&mut self, paths: &[String]) {
        let now = now_secs();
        for path in paths {
            self.touch(path, SELECT_WEIGHT, now);
            if let Some(entry) = self.data.entries.get_mut(path.as_str()) {
                entry.select_count += 1;
            }
        }
    }

    /// Log a query event to the ring buffer (capped at 500 entries).
    pub fn record_query(&mut self, event: QueryEvent) {
        self.data.recent_queries.push_back(event);
        while self.data.recent_queries.len() > MAX_QUERY_HISTORY {
            self.data.recent_queries.pop_front();
        }
    }

    /// Re-rank `hits` in-place: frecency-scored files first (descending), unscored maintain order.
    pub fn rerank_hits(&self, hits: &mut [SearchHit]) {
        let now = now_secs();
        hits.sort_by(|a, b| {
            let sa = self.score_at(hit_path(a), now);
            let sb = self.score_at(hit_path(b), now);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Flush to disk atomically (write to `.tmp`, then rename).
    pub fn flush(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Touch an entry: apply decay then add `weight`.
    fn touch(&mut self, path: &str, weight: f64, now: i64) {
        let entry = self
            .data
            .entries
            .entry(path.to_string())
            .or_insert_with(|| FrecencyEntry {
                score: 0.0,
                last_access_secs: now,
                result_count: 0,
                select_count: 0,
            });
        let dt = (now - entry.last_access_secs).max(0) as f64;
        entry.score = entry.score * decay(dt, self.data.half_life_secs) + weight;
        entry.last_access_secs = now;
    }

    /// Score at a specific `now` without modifying state.
    fn score_at(&self, path: &str, now: i64) -> f64 {
        if let Some(entry) = self.data.entries.get(path) {
            let dt = (now - entry.last_access_secs).max(0) as f64;
            entry.score * decay(dt, self.data.half_life_secs)
        } else {
            0.0
        }
    }
}

fn hit_path(hit: &SearchHit) -> &str {
    match hit {
        SearchHit::Content { path, .. } => path,
        SearchHit::Path { path } => path,
    }
}

/// Exponential decay: 2^(-dt / half_life).
fn decay(dt_secs: f64, half_life_secs: f64) -> f64 {
    f64::powf(2.0, -dt_secs / half_life_secs)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_hit(path: &str) -> SearchHit {
        SearchHit::Path {
            path: path.to_string(),
        }
    }

    #[test]
    fn decay_formula_half_life() {
        // After exactly one half-life, score should halve.
        let score = 8.0 * decay(HALF_LIFE_SECS, HALF_LIFE_SECS);
        assert!((score - 4.0).abs() < 1e-10, "score={score}");
    }

    #[test]
    fn decay_formula_30_days() {
        // After 30 days (~4.3 half-lives) score should be ~5% of original.
        let thirty_days = 30.0 * 86_400.0;
        let frac = decay(thirty_days, HALF_LIFE_SECS);
        assert!(frac < 0.06, "frac={frac}");
        assert!(frac > 0.04, "frac={frac}");
    }

    #[test]
    fn empty_store_score_is_zero() {
        let dir = TempDir::new().unwrap();
        let store = FrecencyStore::open(dir.path());
        assert_eq!(store.score_for("src/main.rs"), 0.0);
    }

    #[test]
    fn record_results_bumps_score() {
        let dir = TempDir::new().unwrap();
        let mut store = FrecencyStore::open(dir.path());
        let hits = vec![make_hit("src/main.rs"), make_hit("src/lib.rs")];
        store.record_results(&hits);
        assert!(store.score_for("src/main.rs") > 0.0);
        assert!(store.score_for("src/lib.rs") > 0.0);
        assert_eq!(store.data.entries["src/main.rs"].result_count, 1);
    }

    #[test]
    fn record_select_higher_weight_than_result() {
        let dir = TempDir::new().unwrap();
        let mut store = FrecencyStore::open(dir.path());
        // Same number of touches
        store.record_results(&[make_hit("a")]);
        store.record_select(&["b".to_string()]);
        let sa = store.score_for("a");
        let sb = store.score_for("b");
        assert!(
            sb > sa,
            "select weight should exceed result weight: a={sa} b={sb}"
        );
    }

    #[test]
    fn rerank_puts_high_score_first() {
        let dir = TempDir::new().unwrap();
        let mut store = FrecencyStore::open(dir.path());
        // "b" selected three times, "a" only once as result
        store.record_select(&["b".to_string(), "b".to_string(), "b".to_string()]);
        store.record_results(&[make_hit("a")]);

        let mut hits = vec![make_hit("a"), make_hit("b"), make_hit("c")];
        store.rerank_hits(&mut hits);
        assert_eq!(hit_path(&hits[0]), "b");
        // "c" (unknown) preserves relative order with "a" but sorts last
    }

    #[test]
    fn flush_and_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut store = FrecencyStore::open(dir.path());
        store.record_results(&[make_hit("src/main.rs")]);
        store.flush().unwrap();

        let loaded = FrecencyStore::open(dir.path());
        assert!(!loaded.is_empty());
        assert!(loaded.score_for("src/main.rs") > 0.0);
    }

    #[test]
    fn query_ring_buffer_capped() {
        let dir = TempDir::new().unwrap();
        let mut store = FrecencyStore::open(dir.path());
        for i in 0..600 {
            store.record_query(QueryEvent {
                timestamp_secs: i,
                pattern: format!("pat{i}"),
                kind: "literal".to_string(),
                result_paths: vec![],
                selected_paths: vec![],
            });
        }
        assert_eq!(store.data.recent_queries.len(), MAX_QUERY_HISTORY);
    }
}
