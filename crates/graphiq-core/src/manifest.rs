use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::search::SearchMode;

pub const MANIFEST_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Ready,
    Stale,
    Missing,
}

impl std::fmt::Display for ArtifactStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactStatus::Ready => write!(f, "ready"),
            ArtifactStatus::Stale => write!(f, "stale"),
            ArtifactStatus::Missing => write!(f, "missing"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMap {
    pub fts: ArtifactStatus,
    pub cruncher: ArtifactStatus,
}

impl ArtifactMap {
    pub fn all_missing() -> Self {
        Self {
            fts: ArtifactStatus::Missing,
            cruncher: ArtifactStatus::Missing,
        }
    }

    pub fn all_ready() -> Self {
        Self {
            fts: ArtifactStatus::Ready,
            cruncher: ArtifactStatus::Ready,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreshnessHash {
    pub symbol_count: i64,
    pub edge_count: i64,
    pub file_count: i64,
}

impl FreshnessHash {
    pub fn from_db(db: &crate::db::GraphDb) -> Self {
        let stats = db.stats().unwrap_or(crate::db::DbStats {
            files: 0,
            symbols: 0,
            edges: 0,
            file_edges: 0,
            schema_version: "0".into(),
        });
        Self {
            symbol_count: stats.symbols,
            edge_count: stats.edges,
            file_count: stats.files,
        }
    }

    pub fn is_stale_vs(&self, other: &FreshnessHash) -> bool {
        self.symbol_count != other.symbol_count
            || self.edge_count != other.edge_count
            || self.file_count != other.file_count
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub freshness: FreshnessHash,
    pub indexed_at: String,
    pub symbols: i64,
    pub edges: i64,
    pub files: i64,
    pub artifacts: ArtifactMap,
    pub active_search_mode: String,
    pub best_available_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downgrade_reasons: Option<Vec<String>>,
}

impl Manifest {
    pub fn compute_active_mode(artifacts: &ArtifactMap) -> SearchMode {
        if artifacts.fts == ArtifactStatus::Ready
            && artifacts.cruncher == ArtifactStatus::Ready
        {
            SearchMode::GraphWalk
        } else {
            SearchMode::Fts
        }
    }

    pub fn compute_downgrade_reasons(artifacts: &ArtifactMap) -> Vec<String> {
        let mut reasons = Vec::new();
        if artifacts.cruncher != ArtifactStatus::Ready {
            reasons.push(format!(
                "cruncher index {} — GraphWalk mode unavailable",
                artifacts.cruncher
            ));
        }
        reasons
    }
}

pub fn manifest_path(db_dir: &Path) -> std::path::PathBuf {
    db_dir.join("manifest.json")
}

pub fn write_manifest(db_dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let path = manifest_path(db_dir);
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("serialize manifest: {e}"))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("write manifest {}: {e}", path.display()))
}

pub fn read_manifest(db_dir: &Path) -> Result<Option<Manifest>, String> {
    let path = manifest_path(db_dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read manifest {}: {e}", path.display()))?;
    let manifest: Manifest = serde_json::from_str(&content)
        .map_err(|e| format!("parse manifest {}: {e}", path.display()))?;
    Ok(Some(manifest))
}

pub fn check_artifact_freshness(
    db: &crate::db::GraphDb,
    manifest: &Manifest,
) -> ArtifactMap {
    let current = FreshnessHash::from_db(db);
    let topology_changed = manifest.freshness.is_stale_vs(&current);

    let mut artifacts = manifest.artifacts.clone();

    if topology_changed {
        if artifacts.cruncher == ArtifactStatus::Ready {
            artifacts.cruncher = ArtifactStatus::Stale;
        }
    }

    if current.symbol_count > 0 {
        artifacts.fts = ArtifactStatus::Ready;
    }

    artifacts
}

pub fn build_manifest(
    db: &crate::db::GraphDb,
    artifacts: ArtifactMap,
) -> Manifest {
    let stats = db.stats().unwrap_or(crate::db::DbStats {
        files: 0,
        symbols: 0,
        edges: 0,
        file_edges: 0,
        schema_version: "0".into(),
    });

    let active_mode = Manifest::compute_active_mode(&artifacts);
    let best_available = active_mode;
    let reasons = Manifest::compute_downgrade_reasons(&artifacts);

    Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        freshness: FreshnessHash::from_db(db),
        indexed_at: chrono_now(),
        symbols: stats.symbols,
        edges: stats.edges,
        files: stats.files,
        artifacts,
        active_search_mode: active_mode.to_string(),
        best_available_mode: best_available.to_string(),
        downgrade_reasons: if reasons.is_empty() {
            None
        } else {
            Some(reasons)
        },
    }
}

pub fn build_manifest_all_ready(db: &crate::db::GraphDb) -> Manifest {
    build_manifest(db, ArtifactMap::all_ready())
}

fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_status_all_ready_gives_graphwalk() {
        let artifacts = ArtifactMap::all_ready();
        assert_eq!(Manifest::compute_active_mode(&artifacts), SearchMode::GraphWalk);
    }

    #[test]
    fn test_artifact_status_missing_cruncher_gives_fts() {
        let mut artifacts = ArtifactMap::all_ready();
        artifacts.cruncher = ArtifactStatus::Missing;
        assert_eq!(Manifest::compute_active_mode(&artifacts), SearchMode::Fts);
    }

    #[test]
    fn test_artifact_status_all_missing_gives_fts() {
        let artifacts = ArtifactMap::all_missing();
        assert_eq!(Manifest::compute_active_mode(&artifacts), SearchMode::Fts);
    }

    #[test]
    fn test_downgrade_reasons() {
        let mut artifacts = ArtifactMap::all_ready();
        artifacts.cruncher = ArtifactStatus::Missing;
        let reasons = Manifest::compute_downgrade_reasons(&artifacts);
        assert!(reasons.iter().any(|r| r.contains("cruncher")));
    }

    #[test]
    fn test_freshness_hash_stale_detection() {
        let a = FreshnessHash { symbol_count: 100, edge_count: 50, file_count: 10 };
        let b = FreshnessHash { symbol_count: 101, edge_count: 50, file_count: 10 };
        assert!(a.is_stale_vs(&b));
        assert!(!a.is_stale_vs(&a));
    }

    #[test]
    fn test_write_and_read_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            freshness: FreshnessHash { symbol_count: 100, edge_count: 50, file_count: 10 },
            indexed_at: "12345".into(),
            symbols: 100,
            edges: 50,
            files: 10,
            artifacts: ArtifactMap::all_ready(),
            active_search_mode: "GraphWalk".into(),
            best_available_mode: "GraphWalk".into(),
            downgrade_reasons: None,
        };
        write_manifest(tmp.path(), &manifest).unwrap();
        let read = read_manifest(tmp.path()).unwrap().unwrap();
        assert_eq!(read.schema_version, MANIFEST_SCHEMA_VERSION);
        assert_eq!(read.symbols, 100);
        assert_eq!(read.artifacts.cruncher, ArtifactStatus::Ready);
    }

    #[test]
    fn test_read_manifest_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_manifest(tmp.path()).unwrap().is_none());
    }
}
