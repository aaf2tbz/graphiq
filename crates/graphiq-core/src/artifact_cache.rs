use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cruncher::HoloIndex;
use crate::spectral::PredictiveModel;

const CACHE_DIR: &str = "cache";
const ZSTD_LEVEL: i32 = 3;
const PREDICTIVE_TOP_K: usize = 200;

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheManifest {
    symbol_count: i64,
    edge_count: i64,
    file_count: i64,
}

impl CacheManifest {
    fn from_db(db: &crate::db::GraphDb) -> Self {
        let stats = db.stats().unwrap_or_else(|_| crate::db::DbStats {
            files: 0, symbols: 0, edges: 0, file_edges: 0, schema_version: String::new(),
        });
        Self {
            symbol_count: stats.symbols,
            edge_count: stats.edges,
            file_count: stats.files,
        }
    }

    fn is_fresh(&self, db: &crate::db::GraphDb) -> bool {
        let current = Self::from_db(db);
        self.symbol_count == current.symbol_count
            && self.edge_count == current.edge_count
            && self.file_count == current.file_count
    }
}

fn cache_dir(db_dir: &Path) -> PathBuf {
    db_dir.join(CACHE_DIR)
}

fn cache_path(db_dir: &Path, name: &str) -> PathBuf {
    cache_dir(db_dir).join(format!("{name}.bin.zst"))
}

fn save_compressed(db_dir: &Path, name: &str, data: &[u8]) {
    let _ = fs::create_dir_all(cache_dir(db_dir));
    let compressed = zstd::encode_all(data, ZSTD_LEVEL).unwrap();
    let _ = fs::write(cache_path(db_dir, name), compressed);
}

fn load_decompressed(db_dir: &Path, name: &str) -> Option<Vec<u8>> {
    let compressed = fs::read(cache_path(db_dir, name)).ok()?;
    zstd::decode_all(&compressed[..]).ok()
}

pub struct ArtifactCache {
    db_dir: PathBuf,
    fresh: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct HoloF32Cache {
    n_symbols: usize,
    holo_dim: usize,
    data: Vec<f32>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PredictiveCompactCache {
    vocab: Vec<String>,
    background_f32: Vec<f32>,
    symbol_ids: Vec<i64>,
    top_per_symbol: Vec<Vec<(u32, f32)>>,
}

impl ArtifactCache {
    pub fn new(db_dir: &Path, db: &crate::db::GraphDb) -> Self {
        let manifest_path = cache_dir(db_dir).join("manifest.json");
        let manifest = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<CacheManifest>(&s).ok());
        let fresh = manifest.as_ref().map_or(false, |m| m.is_fresh(db));
        Self {
            db_dir: db_dir.to_path_buf(),
            fresh,
        }
    }

    pub fn is_fresh(&self) -> bool {
        self.fresh
    }

    pub fn load<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T> {
        if !self.fresh { return None; }
        let data = load_decompressed(&self.db_dir, name)?;
        bincode::deserialize(&data).ok()
    }

    pub fn save<T: serde::Serialize>(&self, name: &str, val: &T) {
        let data = bincode::serialize(val).unwrap();
        save_compressed(&self.db_dir, name, &data);
    }

    pub fn save_manifest(&mut self, db: &crate::db::GraphDb) {
        let manifest = CacheManifest::from_db(db);
        let _ = fs::create_dir_all(cache_dir(&self.db_dir));
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let _ = fs::write(cache_dir(&self.db_dir).join("manifest.json"), json);
        self.fresh = true;
    }

    pub fn invalidate(&self) {
        let dir = cache_dir(&self.db_dir);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    pub fn load_holo(&self) -> Option<HoloIndex> {
        if !self.fresh { return None; }
        let cache: HoloF32Cache = self.load("holo_f32")?;
        let mut name_holos = Vec::with_capacity(cache.n_symbols);
        for i in 0..cache.n_symbols {
            let start = i * cache.holo_dim;
            let end = start + cache.holo_dim;
            let v: Vec<f64> = cache.data[start..end].iter().map(|&x| x as f64).collect();
            name_holos.push(v);
        }
        Some(HoloIndex {
            name_holos,
            term_freq: HashMap::new(),
        })
    }

    pub fn save_holo(&self, hi: &HoloIndex) {
        let n = hi.name_holos.len();
        let dim = hi.name_holos.first().map(|v| v.len()).unwrap_or(0);
        let data: Vec<f32> = hi.name_holos.iter()
            .flat_map(|v| v.iter().map(|&x| x as f32))
            .collect();
        self.save("holo_f32", &HoloF32Cache { n_symbols: n, holo_dim: dim, data });
    }

    pub fn load_predictive(&self) -> Option<PredictiveModel> {
        if !self.fresh { return None; }
        let cache: PredictiveCompactCache = self.load("predictive_compact")?;

        let sym_id_to_idx: HashMap<i64, usize> = cache.symbol_ids.iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();

        let background_terms: HashMap<String, f64> = cache.vocab.iter()
            .zip(cache.background_f32.iter())
            .map(|(t, &v)| (t.clone(), v as f64))
            .collect();

        let conditional_terms: Vec<HashMap<String, f64>> = cache.top_per_symbol.iter()
            .map(|entries| {
                entries.iter()
                    .map(|&(idx, val)| (cache.vocab[idx as usize].clone(), val as f64))
                    .collect()
            })
            .collect();

        Some(PredictiveModel {
            symbol_ids: cache.symbol_ids,
            sym_id_to_idx,
            conditional_terms,
            background_terms,
        })
    }

    pub fn save_predictive(&self, model: &PredictiveModel) {
        let mut vocab: Vec<String> = model.background_terms.keys().cloned().collect();
        vocab.sort();

        let vocab_idx: HashMap<String, u32> = vocab.iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), i as u32))
            .collect();

        let background_f32: Vec<f32> = vocab.iter()
            .map(|t| model.background_terms.get(t).copied().unwrap_or(0.0) as f32)
            .collect();

        let top_per_symbol: Vec<Vec<(u32, f32)>> = model.conditional_terms.iter()
            .map(|cond| {
                let mut scored: Vec<(f64, u32, f64)> = Vec::new();
                for (t, &p) in cond {
                    if let Some(&idx) = vocab_idx.get(t) {
                        let bg = model.background_terms.get(t).copied().unwrap_or(1e-6);
                        let diff = (p - bg).abs();
                        scored.push((diff, idx, p));
                    }
                }
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                scored.truncate(PREDICTIVE_TOP_K);
                scored.into_iter().map(|(_, idx, val)| (idx, val as f32)).collect()
            })
            .collect();

        self.save("predictive_compact", &PredictiveCompactCache {
            vocab,
            background_f32,
            symbol_ids: model.symbol_ids.clone(),
            top_per_symbol,
        });
    }
}
