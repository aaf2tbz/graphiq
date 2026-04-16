use std::sync::Mutex;
use std::time::Instant;

use dashmap::DashMap;
use lru::LruCache;

use crate::db::GraphDb;
use crate::edge::{BlastDirection, BlastEntry};
use crate::rerank::ScoredSymbol;
use crate::symbol::Symbol;

#[derive(Debug, Clone)]
pub struct Neighborhood {
    pub symbol_id: i64,
    pub callers: Vec<(Symbol, f64)>,
    pub callees: Vec<(Symbol, f64)>,
    pub members: Vec<Symbol>,
    pub container: Option<Symbol>,
    pub implementors: Vec<Symbol>,
    pub parents: Vec<Symbol>,
    pub tests: Vec<Symbol>,
    pub loaded_at: Instant,
}

#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub symbol: Symbol,
    pub source: String,
    pub signature_context: String,
    pub callers_summary: String,
    pub callees_summary: String,
    pub test_summary: Option<String>,
    pub file_context: String,
    pub assembled_at: Instant,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct BlastKey {
    pub symbol_id: i64,
    pub direction: BlastDirection,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub neighborhood_count: usize,
    pub assembled_count: usize,
    pub result_entries: usize,
    pub blast_entries: usize,
    pub source_entries: usize,
    pub result_hits: u64,
    pub result_misses: u64,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_neighborhoods: usize,
    pub max_assembled: usize,
    pub max_results: usize,
    pub max_blast: usize,
    pub max_source: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_neighborhoods: 10_000,
            max_assembled: 2_000,
            max_results: 500,
            max_blast: 1_000,
            max_source: 5_000,
        }
    }
}

pub struct HotCache {
    neighborhoods: DashMap<i64, Neighborhood>,
    assembled: DashMap<i64, AssembledContext>,
    results: Mutex<LruCache<u64, Vec<ScoredSymbol>>>,
    blast: DashMap<BlastKey, Vec<BlastEntry>>,
    source: DashMap<i64, String>,
    config: CacheConfig,
    result_hits: Mutex<u64>,
    result_misses: Mutex<u64>,
}

impl HotCache {
    pub fn new(config: CacheConfig) -> Self {
        let max_results = std::num::NonZero::new(config.max_results.max(1)).unwrap();
        Self {
            neighborhoods: DashMap::new(),
            assembled: DashMap::new(),
            results: Mutex::new(LruCache::new(max_results)),
            blast: DashMap::new(),
            source: DashMap::new(),
            config,
            result_hits: Mutex::new(0),
            result_misses: Mutex::new(0),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(CacheConfig::default())
    }

    // --- Neighborhoods ---

    pub fn get_neighborhood(&self, symbol_id: i64) -> Option<Neighborhood> {
        self.neighborhoods
            .get(&symbol_id)
            .map(|r| r.value().clone())
    }

    pub fn put_neighborhood(&self, neighborhood: Neighborhood) {
        if self.neighborhoods.len() >= self.config.max_neighborhoods {
            let oldest = self
                .neighborhoods
                .iter()
                .min_by_key(|e| e.value().loaded_at)
                .map(|e| e.key().clone());
            if let Some(id) = oldest {
                self.neighborhoods.remove(&id);
            }
        }
        self.neighborhoods
            .insert(neighborhood.symbol_id, neighborhood);
    }

    pub fn load_neighborhood(&self, db: &GraphDb, symbol_id: i64) -> Option<Neighborhood> {
        if let Some(n) = self.get_neighborhood(symbol_id) {
            return Some(n);
        }

        let _sym = db.get_symbol(symbol_id).ok()??;

        let mut callers = Vec::new();
        let mut callees = Vec::new();
        let mut members = Vec::new();
        let mut container = None;
        let mut implementors = Vec::new();
        let mut parents = Vec::new();
        let mut tests = Vec::new();

        if let Ok(edges_from) = db.edges_from(symbol_id) {
            for e in &edges_from {
                if let Some(target) = db.get_symbol(e.target_id).unwrap_or(None) {
                    match e.kind {
                        crate::edge::EdgeKind::Calls => {
                            callees.push((target, e.weight));
                        }
                        crate::edge::EdgeKind::Contains => {
                            members.push(target);
                        }
                        crate::edge::EdgeKind::Implements | crate::edge::EdgeKind::Extends => {
                            parents.push(target);
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Ok(edges_to) = db.edges_to(symbol_id) {
            for e in &edges_to {
                if let Some(source) = db.get_symbol(e.source_id).unwrap_or(None) {
                    match e.kind {
                        crate::edge::EdgeKind::Calls => {
                            callers.push((source, e.weight));
                        }
                        crate::edge::EdgeKind::Contains => {
                            container = Some(source);
                        }
                        crate::edge::EdgeKind::Implements => {
                            implementors.push(source);
                        }
                        crate::edge::EdgeKind::Tests => {
                            tests.push(source);
                        }
                        _ => {}
                    }
                }
            }
        }

        let neighborhood = Neighborhood {
            symbol_id,
            callers,
            callees,
            members,
            container,
            implementors,
            parents,
            tests,
            loaded_at: Instant::now(),
        };

        self.put_neighborhood(neighborhood.clone());
        Some(neighborhood)
    }

    // --- Result Cache ---

    pub fn get_results(&self, query_hash: u64) -> Option<Vec<ScoredSymbol>> {
        let mut cache = self.results.lock().ok()?;
        if let Some(results) = cache.get(&query_hash) {
            if let Ok(mut hits) = self.result_hits.lock() {
                *hits += 1;
            }
            Some(results.clone())
        } else {
            if let Ok(mut misses) = self.result_misses.lock() {
                *misses += 1;
            }
            None
        }
    }

    pub fn put_results(&self, query_hash: u64, results: Vec<ScoredSymbol>) {
        if let Ok(mut cache) = self.results.lock() {
            cache.put(query_hash, results);
        }
    }

    pub fn compute_query_hash(query: &str, top_k: usize) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        query.hash(&mut hasher);
        top_k.hash(&mut hasher);
        hasher.finish()
    }

    // --- Blast Cache ---

    pub fn get_blast(&self, key: &BlastKey) -> Option<Vec<BlastEntry>> {
        self.blast.get(key).map(|r| r.value().clone())
    }

    pub fn put_blast(&self, key: BlastKey, entries: Vec<BlastEntry>) {
        if self.blast.len() >= self.config.max_blast {
            let oldest_key = self.blast.iter().next().map(|e| e.key().clone());
            if let Some(k) = oldest_key {
                self.blast.remove(&k);
            }
        }
        self.blast.insert(key, entries);
    }

    // --- Source Cache ---

    pub fn get_source(&self, symbol_id: i64) -> Option<String> {
        self.source.get(&symbol_id).map(|r| r.value().clone())
    }

    pub fn put_source(&self, symbol_id: i64, source: String) {
        if self.source.len() >= self.config.max_source {
            let oldest_key = self.source.iter().next().map(|e| e.key().clone());
            if let Some(k) = oldest_key {
                self.source.remove(&k);
            }
        }
        self.source.insert(symbol_id, source);
    }

    // --- Assembled Context ---

    pub fn get_assembled(&self, symbol_id: i64) -> Option<AssembledContext> {
        self.assembled.get(&symbol_id).map(|r| r.value().clone())
    }

    pub fn put_assembled(&self, ctx: AssembledContext) {
        if self.assembled.len() >= self.config.max_assembled {
            let oldest_key = self.assembled.iter().next().map(|e| e.key().clone());
            if let Some(k) = oldest_key {
                self.assembled.remove(&k);
            }
        }
        self.assembled.insert(ctx.symbol.id, ctx);
    }

    // --- Invalidation ---

    pub fn invalidate_file(&self, file_id: i64, db: &GraphDb) {
        if let Ok(symbols) = db.symbols_by_file(file_id) {
            for sym in &symbols {
                self.neighborhoods.remove(&sym.id);
                self.source.remove(&sym.id);
                self.assembled.remove(&sym.id);
                self.blast.retain(|k, _| k.symbol_id != sym.id);
            }
        }
        if let Ok(mut cache) = self.results.lock() {
            cache.clear();
        }
    }

    pub fn clear(&self) {
        self.neighborhoods.clear();
        self.assembled.clear();
        if let Ok(mut cache) = self.results.lock() {
            cache.clear();
        }
        self.blast.clear();
        self.source.clear();
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            neighborhood_count: self.neighborhoods.len(),
            assembled_count: self.assembled.len(),
            result_entries: self.results.lock().map(|c| c.len()).unwrap_or(0),
            blast_entries: self.blast.len(),
            source_entries: self.source.len(),
            result_hits: self.result_hits.lock().map(|h| *h).unwrap_or(0),
            result_misses: self.result_misses.lock().map(|m| *m).unwrap_or(0),
        }
    }

    pub fn prewarm(&self, db: &GraphDb, top_n: usize) {
        let conn = db.conn();
        let mut stmt =
            match conn.prepare("SELECT id FROM symbols ORDER BY importance DESC LIMIT ?1") {
                Ok(s) => s,
                Err(_) => return,
            };
        let rows: Vec<i64> = match stmt.query_map(rusqlite::params![top_n as i64], |row| row.get(0))
        {
            Ok(r) => r.flatten().collect(),
            Err(_) => return,
        };

        for symbol_id in rows {
            self.load_neighborhood(db, symbol_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{SymbolBuilder, SymbolKind};

    fn setup_db() -> GraphDb {
        let db = GraphDb::open_in_memory().unwrap();
        let fid = db
            .upsert_file("src/main.ts", "typescript", "abc", 1000, 50)
            .unwrap();
        let sym = SymbolBuilder::new(
            fid,
            "main".into(),
            SymbolKind::Function,
            "fn main()".into(),
            "typescript".into(),
        )
        .lines(1, 5)
        .build();
        db.insert_symbol(&sym).unwrap();
        db
    }

    #[test]
    fn test_cache_source() {
        let cache = HotCache::with_defaults();
        cache.put_source(1, "fn main()".into());
        assert_eq!(cache.get_source(1), Some("fn main()".into()));
        assert_eq!(cache.get_source(999), None);
    }

    #[test]
    fn test_cache_results() {
        let cache = HotCache::with_defaults();
        let hash = HotCache::compute_query_hash("test query", 10);
        assert!(cache.get_results(hash).is_none());

        cache.put_results(hash, vec![]);
        assert!(cache.get_results(hash).is_some());
    }

    #[test]
    fn test_cache_invalidation() {
        let db = setup_db();
        let cache = HotCache::with_defaults();
        cache.put_source(1, "source".into());
        cache.invalidate_file(1, &db);
        assert!(cache.get_source(1).is_none());
    }

    #[test]
    fn test_cache_stats() {
        let cache = HotCache::with_defaults();
        cache.put_source(1, "source".into());
        cache.put_source(2, "source2".into());
        let stats = cache.stats();
        assert_eq!(stats.source_entries, 2);
    }

    #[test]
    fn test_load_neighborhood() {
        let db = setup_db();
        let cache = HotCache::with_defaults();
        let n = cache.load_neighborhood(&db, 1);
        assert!(n.is_some());
        let n2 = cache.get_neighborhood(1);
        assert!(n2.is_some());
    }

    #[test]
    fn test_query_hash_deterministic() {
        let h1 = HotCache::compute_query_hash("test", 10);
        let h2 = HotCache::compute_query_hash("test", 10);
        assert_eq!(h1, h2);

        let h3 = HotCache::compute_query_hash("other", 10);
        assert_ne!(h1, h3);
    }
}
