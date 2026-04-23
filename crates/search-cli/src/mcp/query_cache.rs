//! In-process LRU cache for MCP search-tool result envelopes.
//!
//! Each entry is keyed by `tool_name | limit | serde_json(QueryRequest)` and
//! stores the JSON envelope returned by `build_envelope`. Entries expire after
//! a configurable TTL; the whole cache is flushed whenever the index is
//! rebuilt.

#![allow(dead_code)]

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct QueryCache {
    inner: Mutex<Inner>,
    ttl: Duration,
    max_entries: usize,
}

struct Inner {
    entries: HashMap<String, CachedEntry>,
    /// Insertion-order deque used for LRU eviction. The front is the oldest
    /// entry; the back is the most-recently used.
    order: VecDeque<String>,
    hits: u64,
    misses: u64,
}

struct CachedEntry {
    value: Value,
    inserted_at: Instant,
}

impl QueryCache {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                hits: 0,
                misses: 0,
            }),
            ttl,
            max_entries,
        }
    }

    /// Return the cached envelope for `key` if it exists and has not expired.
    /// Moves the entry to the back of the LRU order on hit.
    pub fn get(&self, key: &str) -> Option<Value> {
        let mut g = self.inner.lock().expect("query_cache mutex poisoned");

        // Check existence and expiry before mutating to satisfy the borrow checker.
        let state = match g.entries.get(key) {
            None => None,
            Some(e) if e.inserted_at.elapsed() > self.ttl => Some(Err(())), // expired
            Some(e) => Some(Ok(e.value.clone())),                           // live
        };

        match state {
            None => {
                g.misses += 1;
                None
            }
            Some(Err(())) => {
                // Lazy expiry — remove the entry.
                g.entries.remove(key);
                g.order.retain(|k| k != key);
                g.misses += 1;
                None
            }
            Some(Ok(value)) => {
                // Refresh LRU position.
                g.order.retain(|k| k != key);
                g.order.push_back(key.to_string());
                g.hits += 1;
                Some(value)
            }
        }
    }

    /// Insert or replace an entry. Evicts the LRU entry when at capacity.
    pub fn put(&self, key: String, value: Value) {
        let mut g = self.inner.lock().expect("query_cache mutex poisoned");
        // If already present, refresh in place.
        if g.entries.contains_key(&key) {
            g.order.retain(|k| k != &key);
        } else if g.entries.len() >= self.max_entries {
            // Evict the oldest.
            if let Some(oldest) = g.order.pop_front() {
                g.entries.remove(&oldest);
            }
        }
        g.entries.insert(
            key.clone(),
            CachedEntry {
                value,
                inserted_at: Instant::now(),
            },
        );
        g.order.push_back(key);
    }

    /// Clear all entries (called on reindex).
    pub fn invalidate_all(&self) {
        let mut g = self.inner.lock().expect("query_cache mutex poisoned");
        g.entries.clear();
        g.order.clear();
    }

    /// Return `(hits, misses)` counters since the cache was created.
    #[allow(dead_code)]
    pub fn stats(&self) -> (u64, u64) {
        let g = self.inner.lock().expect("query_cache mutex poisoned");
        (g.hits, g.misses)
    }

    /// Return the number of currently live (possibly expired) entries.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("query_cache mutex poisoned")
            .entries
            .len()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_cache(ttl_ms: u64, max: usize) -> QueryCache {
        QueryCache::new(Duration::from_millis(ttl_ms), max)
    }

    #[test]
    fn miss_then_hit() {
        let c = make_cache(1_000, 16);
        assert!(c.get("k1").is_none());
        c.put("k1".into(), json!({"result": 1}));
        let v = c.get("k1").expect("should hit");
        assert_eq!(v["result"], 1);
    }

    #[test]
    fn ttl_expiry_triggers_miss() {
        let c = make_cache(1, 16); // 1 ms TTL
        c.put("k".into(), json!(42));
        std::thread::sleep(Duration::from_millis(5));
        assert!(c.get("k").is_none(), "should have expired");
    }

    #[test]
    fn invalidate_all_clears() {
        let c = make_cache(1_000, 16);
        c.put("a".into(), json!(1));
        c.put("b".into(), json!(2));
        assert_eq!(c.len(), 2);
        c.invalidate_all();
        assert_eq!(c.len(), 0);
        assert!(c.get("a").is_none());
    }

    #[test]
    fn lru_eviction_at_max_entries() {
        let c = make_cache(1_000, 3);
        c.put("a".into(), json!(1));
        c.put("b".into(), json!(2));
        c.put("c".into(), json!(3));
        // Refresh "a" so it is no longer the oldest.
        c.get("a");
        // Insert "d" — should evict "b" (oldest after "a" was refreshed).
        c.put("d".into(), json!(4));
        assert!(c.get("b").is_none(), "b should have been evicted");
        assert!(c.get("a").is_some(), "a was refreshed so it stays");
        assert!(c.get("c").is_some());
        assert!(c.get("d").is_some());
    }

    #[test]
    fn stats_count_hits_and_misses() {
        let c = make_cache(1_000, 16);
        c.get("x"); // miss
        c.put("x".into(), json!(1));
        c.get("x"); // hit
        c.get("x"); // hit
        let (hits, misses) = c.stats();
        assert_eq!(hits, 2);
        assert_eq!(misses, 1);
    }

    #[test]
    fn put_refreshes_existing_key() {
        let c = make_cache(1_000, 16);
        c.put("k".into(), json!(1));
        c.put("k".into(), json!(2));
        // Size should stay 1.
        assert_eq!(c.len(), 1);
        assert_eq!(c.get("k").unwrap(), json!(2));
    }
}
