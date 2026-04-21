use std::collections::HashMap;

use crate::search::SearchMode;
use crate::query_family::QueryFamily;

#[derive(Debug, Clone)]
pub enum SeedOrigin {
    Bm25,
    NameExpansion,
    FilePathRouter,
    GraphWalk,
    FtsDecomposed,
}

#[derive(Debug, Clone)]
pub struct SeedHit {
    pub symbol_id: i64,
    pub origin: SeedOrigin,
    pub raw_score: f64,
}

#[derive(Debug, Clone)]
pub struct ExpansionStep {
    pub from_symbol_id: i64,
    pub to_symbol_id: i64,
    pub edge_kind: String,
    pub evidence_kind: Option<String>,
    pub heat_contribution: f64,
}

#[derive(Debug, Clone)]
pub struct TraceEdge {
    pub from_symbol_id: i64,
    pub to_symbol_id: i64,
    pub edge_kind: String,
    pub evidence_kind: String,
    pub weight: f64,
}

#[derive(Debug, Clone)]
pub struct TraceScoreBreakdown {
    pub bm25_raw: f64,
    pub coverage_score: f64,
    pub name_score: f64,
    pub walk_evidence: f64,
    pub is_seed: bool,
    pub bm25_locked: bool,
    pub final_score: f64,
}

#[derive(Debug, Clone)]
pub struct ConfidenceReport {
    pub seed_strength: f64,
    pub evidence_channels: usize,
    pub coverage_fraction: f64,
    pub has_name_match: bool,
    pub has_structural_path: bool,
}

impl ConfidenceReport {
    pub fn confidence(&self) -> f64 {
        let mut c = 0.0;
        c += self.seed_strength * 0.3;
        c += (self.evidence_channels as f64 / 5.0).min(1.0) * 0.2;
        c += self.coverage_fraction * 0.2;
        if self.has_name_match { c += 0.15; }
        if self.has_structural_path { c += 0.15; }
        c.min(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct RetrievalTrace {
    pub query_family: QueryFamily,
    pub search_mode: SearchMode,
    pub seeds: Vec<SeedHit>,
    pub expansions: Vec<ExpansionStep>,
    pub evidence_edges: Vec<TraceEdge>,
    pub score: TraceScoreBreakdown,
    pub confidence: ConfidenceReport,
}

impl RetrievalTrace {
    pub fn format_debug(&self, symbol_name: &str) -> String {
        let mut lines = Vec::new();

        lines.push(format!("  trace for '{}':", symbol_name));
        lines.push(format!("    family: {}  mode: {}", self.query_family, self.search_mode));

        let seed_count = self.seeds.len();
        let bm25_seeds = self.seeds.iter().filter(|s| matches!(s.origin, SeedOrigin::Bm25)).count();
        let name_seeds = self.seeds.iter().filter(|s| matches!(s.origin, SeedOrigin::NameExpansion)).count();
        let walk_seeds = self.seeds.iter().filter(|s| matches!(s.origin, SeedOrigin::GraphWalk)).count();
        lines.push(format!("    seeds: {} total ({} bm25, {} name, {} walk)", seed_count, bm25_seeds, name_seeds, walk_seeds));

        if !self.expansions.is_empty() {
            lines.push(format!("    expansions: {} steps", self.expansions.len()));
            for exp in self.expansions.iter().take(5) {
                lines.push(format!("      {} -> {} [{}] heat={:.3}",
                    exp.from_symbol_id, exp.to_symbol_id, exp.edge_kind, exp.heat_contribution));
            }
        }

        if !self.evidence_edges.is_empty() {
            lines.push(format!("    evidence edges: {}", self.evidence_edges.len()));
            for e in self.evidence_edges.iter().take(5) {
                lines.push(format!("      {} -> {} [{}] {}={:.3}",
                    e.from_symbol_id, e.to_symbol_id, e.edge_kind, e.evidence_kind, e.weight));
            }
        }

        let s = &self.score;
        lines.push("    score breakdown:".into());
        lines.push(format!("      bm25={:.3}  coverage={:.3}  name={:.3}", s.bm25_raw, s.coverage_score, s.name_score));
        lines.push(format!("      walk_evidence={:.3}", s.walk_evidence));
        lines.push(format!("      is_seed={}  bm25_locked={}  final={:.3}", s.is_seed, s.bm25_locked, s.final_score));

        let conf = &self.confidence;
        lines.push(format!("    confidence: {:.2} (seed_strength={:.2}, channels={}, coverage={:.2}, name={}, path={})",
            conf.confidence(),
            conf.seed_strength,
            conf.evidence_channels,
            conf.coverage_fraction,
            conf.has_name_match,
            conf.has_structural_path,
        ));

        lines.join("\n")
    }

    pub fn format_why(&self, symbol_name: &str, query: &str) -> String {
        let mut lines = Vec::new();
        lines.push(format!("=== Why did '{}' rank for '{}'? ===", symbol_name, query));
        lines.push(format!("Family: {}  Mode: {}", self.query_family, self.search_mode));
        lines.push(format!("Final score: {:.4}", self.score.final_score));
        lines.push(String::new());

        lines.push("How it was found:".into());
        for seed in &self.seeds {
            let origin = match seed.origin {
                SeedOrigin::Bm25 => "BM25 seed",
                SeedOrigin::NameExpansion => "name match expansion",
                SeedOrigin::FilePathRouter => "file path router",
                SeedOrigin::GraphWalk => "graph walk",
                SeedOrigin::FtsDecomposed => "FTS decomposed",
            };
            lines.push(format!("  {} (raw={:.3})", origin, seed.raw_score));
        }

        lines.push(String::new());
        lines.push("Score composition:".into());
        let s = &self.score;
        if s.is_seed {
            lines.push(format!("  BM25 contribution: {:.3}", s.bm25_raw));
        }
        if s.coverage_score > 0.0 {
            lines.push(format!("  Term coverage: {:.3}", s.coverage_score));
        }
        if s.name_score > 0.0 {
            lines.push(format!("  Name match: {:.3}", s.name_score));
        }
        if s.walk_evidence > 0.0 {
            lines.push(format!("  Walk evidence: {:.3}", s.walk_evidence));
        }
        if s.bm25_locked {
            lines.push("  BM25 lock: YES (top seed promoted to #1)".into());
        }

        if !self.evidence_edges.is_empty() {
            lines.push(String::new());
            lines.push(format!("Evidence chain ({} edges):", self.evidence_edges.len()));
            for e in self.evidence_edges.iter().take(10) {
                lines.push(format!("  [{}] {} ({:.3})", e.evidence_kind, e.edge_kind, e.weight));
            }
        }

        if !self.expansions.is_empty() {
            lines.push(String::new());
            lines.push(format!("Expansion path ({} steps):", self.expansions.len()));
            for exp in self.expansions.iter().take(5) {
                lines.push(format!("  via {} [{}] heat={:.3}",
                    exp.edge_kind,
                    exp.evidence_kind.as_deref().unwrap_or("?"),
                    exp.heat_contribution));
            }
        }

        lines.push(String::new());
        let conf = &self.confidence;
        lines.push(format!("Confidence: {:.0}% — {}", conf.confidence() * 100.0, self.confidence_label()));

        lines.join("\n")
    }

    fn confidence_label(&self) -> &'static str {
        let c = self.confidence.confidence();
        if c >= 0.8 { "high confidence" }
        else if c >= 0.5 { "moderate confidence" }
        else if c >= 0.3 { "low confidence — likely needs verification" }
        else { "very low confidence — result may be spurious" }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TraceCollector {
    pub traces: HashMap<i64, RetrievalTrace>,
}

impl TraceCollector {
    pub fn new() -> Self {
        Self {
            traces: HashMap::new(),
        }
    }
}
