//! In-process search memo for MCP search tools.
//!
//! Unlike the older full-envelope query cache, this store tracks just enough
//! metadata to tell the model to reuse an earlier search result already in
//! conversation context when the daemon can prove nothing relevant changed.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct SearchMemoEntry {
    pub search_id: String,
    pub recorded_generation: u64,
    pub recorded_context_epoch: u64,
    pub matched_paths: Vec<String>,
    pub files_with_matches: u64,
    pub total_line_matches: u64,
    pub strategy: String,
}

pub struct SearchMemo {
    inner: Mutex<Inner>,
    max_entries: usize,
}

struct Inner {
    entries: HashMap<String, SearchMemoEntry>,
    order: VecDeque<String>,
    next_id: u64,
}

impl SearchMemo {
    pub fn new(max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                next_id: 0,
            }),
            max_entries,
        }
    }

    pub fn get(&self, key: &str) -> Option<SearchMemoEntry> {
        let mut guard = self.inner.lock().expect("search_memo mutex poisoned");
        let entry = guard.entries.get(key).cloned()?;
        guard.order.retain(|existing| existing != key);
        guard.order.push_back(key.to_string());
        Some(entry)
    }

    pub fn put(&self, key: String, mut entry: SearchMemoEntry) -> SearchMemoEntry {
        let mut guard = self.inner.lock().expect("search_memo mutex poisoned");
        if let Some(existing) = guard.entries.get(&key) {
            entry.search_id = existing.search_id.clone();
            guard.order.retain(|existing_key| existing_key != &key);
        } else {
            guard.next_id += 1;
            entry.search_id = format!("search-{:06}", guard.next_id);
            if guard.entries.len() >= self.max_entries
                && let Some(oldest) = guard.order.pop_front()
            {
                guard.entries.remove(&oldest);
            }
        }
        guard.entries.insert(key.clone(), entry.clone());
        guard.order.push_back(key);
        entry
    }

    pub fn invalidate_all(&self) {
        let mut guard = self.inner.lock().expect("search_memo mutex poisoned");
        guard.entries.clear();
        guard.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> SearchMemoEntry {
        SearchMemoEntry {
            search_id: String::new(),
            recorded_generation: 7,
            recorded_context_epoch: 2,
            matched_paths: vec!["src/lib.rs".into()],
            files_with_matches: 1,
            total_line_matches: 2,
            strategy: "triseek_indexed".into(),
        }
    }

    #[test]
    fn assigns_stable_search_id() {
        let memo = SearchMemo::new(16);
        let first = memo.put("k".into(), sample_entry());
        let second = memo.put("k".into(), sample_entry());
        assert_eq!(first.search_id, second.search_id);
    }

    #[test]
    fn evicts_oldest_entry() {
        let memo = SearchMemo::new(2);
        memo.put("a".into(), sample_entry());
        memo.put("b".into(), sample_entry());
        memo.put("c".into(), sample_entry());
        assert!(memo.get("a").is_none());
        assert!(memo.get("b").is_some());
        assert!(memo.get("c").is_some());
    }

    #[test]
    fn invalidate_all_clears_entries() {
        let memo = SearchMemo::new(2);
        memo.put("a".into(), sample_entry());
        memo.invalidate_all();
        assert!(memo.get("a").is_none());
    }
}
