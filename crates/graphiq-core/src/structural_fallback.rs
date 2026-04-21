use std::collections::{HashMap, HashSet};

use crate::cruncher::{CruncherIndex, Edge};
use crate::spectral::ChannelFingerprint;

pub struct SnpConfig {
    pub diffusion_threshold: f64,
    pub max_expand_per_seed: usize,
    pub max_depth: usize,
    pub role_match_weight: f64,
    pub distance_decay: f64,
}

impl Default for SnpConfig {
    fn default() -> Self {
        Self {
            diffusion_threshold: 0.6,
            max_expand_per_seed: 40,
            max_depth: 3,
            role_match_weight: 3.0,
            distance_decay: 0.5,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum StructuralRole {
    Orchestrator,
    Library,
    Boundary,
    Worker,
    Isolate,
    Unknown,
}

impl StructuralRole {
    pub fn from_str(s: &str) -> Self {
        match s {
            "orchestrator" => Self::Orchestrator,
            "library" => Self::Library,
            "boundary" => Self::Boundary,
            "worker" => Self::Worker,
            "isolate" => Self::Isolate,
            _ => Self::Unknown,
        }
    }

    fn compatibility(self, other: Self) -> f64 {
        if self == other {
            return 1.0;
        }
        match (self, other) {
            (Self::Orchestrator, Self::Boundary) | (Self::Boundary, Self::Orchestrator) => 0.7,
            (Self::Worker, Self::Isolate) | (Self::Isolate, Self::Worker) => 0.6,
            (Self::Orchestrator, Self::Worker) | (Self::Worker, Self::Orchestrator) => 0.3,
            (Self::Library, Self::Boundary) | (Self::Boundary, Self::Library) => 0.5,
            _ => 0.2,
        }
    }
}

pub fn seed_diffusion_score(bm25_seeds: &[(i64, f64)]) -> f64 {
    if bm25_seeds.len() < 3 {
        return 0.0;
    }
    let n = bm25_seeds.len().min(15);
    let scores: Vec<f64> = bm25_seeds[..n].iter().map(|(_, s)| *s).collect();
    let mean: f64 = scores.iter().sum::<f64>() / n as f64;
    if mean < 1e-10 {
        return 1.0;
    }
    let variance: f64 = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();
    let cv = std_dev / mean;
    1.0 / (1.0 + cv)
}

const ORCHESTRATOR_SIGNALS: &[&str] = &[
    "coordinate", "orchestrate", "manage", "lifecycle", "sequence",
    "transition", "state machine", "workflow", "pipeline", "shutdown",
    "startup", "initialize", "bootstrap", "dispatch",
];

const WORKER_SIGNALS: &[&str] = &[
    "read", "write", "flush", "poll", "fill", "drain", "buffered",
    "transfer", "bytes", "stream", "socket", "channel", "send",
    "receive", "recv", "encode", "decode", "parse", "serialize",
    "process", "handle", "consume", "produce",
];

const LIBRARY_SIGNALS: &[&str] = &[
    "check", "validate", "verify", "determine", "compute", "calculate",
    "convert", "transform", "format", "normalize", "resolve", "lookup",
    "is_", "can_", "has_", "should_", "compare",
];

const BOUNDARY_SIGNALS: &[&str] = &[
    "bridge", "adapter", "connector", "interface", "cross", "boundary",
    "between", "connect", "route", "proxy", "translate", "mediat",
];

pub fn infer_query_role(query: &str) -> StructuralRole {
    let lower = query.to_lowercase();

    let mut scores: HashMap<StructuralRole, f64> = HashMap::new();
    for sig in ORCHESTRATOR_SIGNALS {
        if lower.contains(sig) {
            *scores.entry(StructuralRole::Orchestrator).or_default() += 1.0;
        }
    }
    for sig in WORKER_SIGNALS {
        if lower.contains(sig) {
            *scores.entry(StructuralRole::Worker).or_default() += 1.0;
        }
    }
    for sig in LIBRARY_SIGNALS {
        if lower.contains(sig) {
            *scores.entry(StructuralRole::Library).or_default() += 0.8;
        }
    }
    for sig in BOUNDARY_SIGNALS {
        if lower.contains(sig) {
            *scores.entry(StructuralRole::Boundary).or_default() += 1.0;
        }
    }

    if lower.contains("all ") || lower.contains("every ") {
        *scores.entry(StructuralRole::Boundary).or_default() += 1.5;
    }

    if lower.contains("how does") || lower.contains("what controls") {
        *scores.entry(StructuralRole::Orchestrator).or_default() += 0.5;
    }

    scores
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(role, _)| role)
        .unwrap_or(StructuralRole::Unknown)
}

pub struct SnpCandidate {
    pub idx: usize,
    pub snp_score: f64,
    pub role_match: f64,
    pub depth: usize,
    pub source_seeds: HashSet<usize>,
}

pub fn structural_neighborhood_expansion(
    seed_indices: &[usize],
    idx: &CruncherIndex,
    fingerprints: Option<&[ChannelFingerprint]>,
    fp_id_to_idx: Option<&HashMap<i64, usize>>,
    query_role: StructuralRole,
    config: &SnpConfig,
    existing_candidates: &HashSet<usize>,
) -> Vec<SnpCandidate> {
    let max_seeds = seed_indices.len().min(10);
    let fp_role_of = |sym_id: i64| -> StructuralRole {
        if let (Some(fps), Some(fp_map)) = (fingerprints, fp_id_to_idx) {
            if let Some(&fi) = fp_map.get(&sym_id) {
                if fi < fps.len() {
                    return StructuralRole::from_str(&fps[fi].role);
                }
            }
        }
        StructuralRole::Unknown
    };

    let deg_of = |i: usize| -> f64 {
        let out_deg = idx.outgoing.get(i).map(|e| e.len()).unwrap_or(0) as f64;
        let in_deg = idx.incoming.get(i).map(|e| e.len()).unwrap_or(0) as f64;
        out_deg + in_deg
    };

    let mut candidates: HashMap<usize, SnpCandidate> = HashMap::new();

    for &seed_i in seed_indices.iter().take(max_seeds) {
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(seed_i);
        let mut queue: VecDeque<(usize, f64, usize)> = VecDeque::new();

        let edges: Vec<&Edge> = idx.outgoing
            .get(seed_i)
            .map(|e| e.iter().take(15).collect())
            .unwrap_or_default();
        for edge in &edges {
            if !visited.contains(&edge.target) && !existing_candidates.contains(&edge.target) {
                visited.insert(edge.target);
                queue.push_back((edge.target, edge.weight, 1));
            }
        }
        let in_edges: Vec<&Edge> = idx.incoming
            .get(seed_i)
            .map(|e| e.iter().take(15).collect())
            .unwrap_or_default();
        for edge in &in_edges {
            if !visited.contains(&edge.target) && !existing_candidates.contains(&edge.target) {
                visited.insert(edge.target);
                queue.push_back((edge.target, edge.weight, 1));
            }
        }

        let mut expanded = 0usize;
        while let Some((neighbor_i, edge_w, depth)) = queue.pop_front() {
            if depth > config.max_depth || expanded >= config.max_expand_per_seed {
                break;
            }

            let sym_id = idx.symbol_ids[neighbor_i];
            let neighbor_role = fp_role_of(sym_id);
            let role_compat = if query_role != StructuralRole::Unknown {
                query_role.compatibility(neighbor_role)
            } else {
                0.5
            };

            if role_compat < 0.15 {
                expanded += 1;
                continue;
            }

            let deg = deg_of(neighbor_i);
            let deg_norm = (deg / 30.0).min(1.0);
            let proximity = config.distance_decay.powi(depth as i32);

            let snp_score = role_compat * config.role_match_weight * proximity * edge_w * (0.5 + 0.5 * deg_norm);

            let entry = candidates.entry(neighbor_i).or_insert_with(|| {
                SnpCandidate {
                    idx: neighbor_i,
                    snp_score: 0.0,
                    role_match: 0.0,
                    depth,
                    source_seeds: HashSet::new(),
                }
            });

            entry.snp_score = entry.snp_score.max(snp_score);
            entry.role_match = entry.role_match.max(role_compat);
            entry.source_seeds.insert(seed_i);
            expanded += 1;

            if depth < config.max_depth {
                let next_edges: Vec<&Edge> = idx.outgoing
                    .get(neighbor_i)
                    .map(|e| e.iter().take(8).collect())
                    .unwrap_or_default();
                for next_edge in next_edges {
                    if !visited.contains(&next_edge.target) && !existing_candidates.contains(&next_edge.target) {
                        visited.insert(next_edge.target);
                        queue.push_back((next_edge.target, next_edge.weight.min(edge_w), depth + 1));
                    }
                }
                let next_in: Vec<&Edge> = idx.incoming
                    .get(neighbor_i)
                    .map(|e| e.iter().take(8).collect())
                    .unwrap_or_default();
                for next_edge in next_in {
                    if !visited.contains(&next_edge.target) && !existing_candidates.contains(&next_edge.target) {
                        visited.insert(next_edge.target);
                        queue.push_back((next_edge.target, next_edge.weight.min(edge_w), depth + 1));
                    }
                }
            }
        }
    }

    let multi_seed_boost = |c: &mut SnpCandidate| {
        if c.source_seeds.len() >= 2 {
            c.snp_score *= 1.0 + 0.3 * c.source_seeds.len() as f64;
        }
    };

    let mut result: Vec<SnpCandidate> = candidates.into_iter().map(|(_, mut c)| {
        multi_seed_boost(&mut c);
        c
    }).collect();
    result.sort_by(|a, b| b.snp_score.partial_cmp(&a.snp_score).unwrap());
    result.truncate(50);
    result
}

use std::collections::VecDeque;
