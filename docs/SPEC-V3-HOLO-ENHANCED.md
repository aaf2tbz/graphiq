# GraphIQ v3: Holographic-Enhanced Simple Pipeline

Port the control structures discovered during holographic experimentation into the v2
simple pipeline. Zero FFT, zero new disk artifacts (beyond CruncherIndex extensions),
zero new dependencies. Target: recover tokio regression, improve NL categories, no
regressions on signetai or esbuild.

## Background

v1's holographic layer produced a real signal (6.8x cosine separation) but required
18GB RAM and 42MB disk cache for marginal NDCG improvement (+0.02-0.05). The v2
simplification removed it all and signetai actually improved, proving the artifact
pipeline was adding noise. But tokio regressed (-0.070 NDCG, -0.122 MRR) because
its generic function names (`run`, `handle`, `poll`) benefited from the holographic
name matching's ability to distinguish similar names through structural composition.

The key insight from the holographic work was never the FFT math. It was:

1. **Confidence gating** — only apply secondary signals when they're strongly confident
2. **Query specificity scaling** — rare-term queries deserve more name-matching weight
3. **Additive-only contributions** — multiplicative boosts just reshuffle, additive can promote
4. **Per-family signal routing** — different query types need different signal combinations
5. **Neighborhood context** — 1-hop graph terms disambiguate generic names

These are pipeline engineering principles, not linear algebra. They can be implemented
with cheap set operations instead of 1024-dim FFT vectors.

## Phase 0: Baseline Capture

Before any changes, capture v2 baseline on all 3 codebases:

```bash
cargo build --release -p graphiq-bench
./target/release/graphiq-bench signetai.db ndcg-20-signetai.json mrr-25-signetai.json
./target/release/graphiq-bench esbuild.db ndcg-20-esbuild.json mrr-25-esbuild.json
./target/release/graphiq-bench tokio.db ndcg-20-tokio.json mrr-25-tokio.json
```

Record NDCG@10, MRR@10, per-category NDCG for all 3. These are the comparison numbers.

## Phase 1: Gated Name Overlap Scoring

### What

Add a cheap name similarity boost to `score_candidates()` using overlap coefficient on
decomposed identifier terms, gated by confidence threshold and scaled by query specificity.

### Why

The holographic cosine measured "how similar are the decomposed terms of the query to
the decomposed terms of the candidate name." A simple overlap coefficient (|intersection|
/ |min_set|) captures the same structural similarity signal without FFT. The 0.25 gate
from v1's holo work is directly reusable — it naturally passes descriptive names
(esbuild) and blocks generic names (tokio) from receiving false-boost noise.

### Changes

#### scoring.rs

Add `name_overlap` field to `Candidate`:
```rust
pub struct Candidate {
    // ... existing fields ...
    pub name_overlap: f64,  // overlap coefficient with query name terms
}
```

Add `name_overlap_gate` and `name_overlap_max_w` to `ScoreConfig`:
```rust
pub struct ScoreConfig {
    // ... existing fields ...
    pub name_overlap_gate: f64,   // default 0.25
    pub name_overlap_max_w: f64,  // default 2.0
}
```

In `score_candidates()`, after base score computation, add:
```rust
let query_specificity = if idf_sum > 0.0 {
    query_terms.iter().filter(|qt| qt.idf > 1.0).count() as f64
        / query_terms.len() as f64
} else {
    0.0
};

let name_overlap_additive = if c.name_overlap > config.name_overlap_gate {
    let excess = (c.name_overlap - config.name_overlap_gate)
        / (1.0 - config.name_overlap_gate);
    config.name_overlap_max_w * query_specificity * excess
} else {
    0.0
};

// Add to base (not multiply)
let raw = (base + name_overlap_additive)
    * coverage_frac.powf(0.3)
    * seed_bonus
    * kb
    * tp;
```

#### cruncher.rs

Add function `compute_name_overlap()`:
```rust
pub fn compute_name_overlap(
    query_terms: &[QueryTerm],
    name_terms: &HashSet<String>,
) -> f64 {
    let query_name_set: HashSet<String> = query_terms
        .iter()
        .flat_map(|qt| qt.variants.iter().cloned())
        .collect();
    
    if query_name_set.is_empty() || name_terms.is_empty() {
        return 0.0;
    }
    
    let intersection = query_name_set
        .iter()
        .filter(|t| name_terms.contains(*t))
        .count();
    
    let min_size = query_name_set.len().min(name_terms.len());
    intersection as f64 / min_size as f64
}
```

This is a set-intersection overlap coefficient — O(|terms|) — not FFT.

#### pipeline.rs

In `unified_search()`, compute `name_overlap` when populating candidates:
```rust
let no = compute_name_overlap(&query_terms, &idx.term_sets[i].name_terms);

candidates.insert(i, Candidate {
    // ... existing fields ...
    name_overlap: no,
});
```

### Verification

- Zero regression on signetai, esbuild
- tokio MRR should recover partially (name overlap helps `acquire_owned` type queries)
- `name_overlap_additive` should be 0.0 for most tokio candidates (generic names
  produce low overlap) — the gate adapts automatically

## Phase 2: Specificity-Weighted Coverage

### What

Use query specificity (fraction of high-IDF terms) as a modifier on coverage scoring,
not just on the name overlap gate.

### Why

Phase 1 uses specificity only to gate the name overlap boost. But the holographic
research showed that specificity is a fundamental control variable — specific queries
(rare terms like "VLQ", "OKLCH") benefit from different scoring weights than broad
queries ("handle request", "run process"). When a query is specific, coverage of
rare terms should matter more. When it's broad, BM25 should dominate.

### Changes

#### scoring.rs

In `score_candidates()`, modify the seed base score:
```rust
// Specificity shifts weight: specific queries trust coverage more,
// broad queries trust BM25 more
let specificity = query_specificity; // computed in Phase 1

let base = if c.is_seed {
    let bm25_w = config.bm25_w * (1.0 - 0.3 * specificity);
    let cov_w = config.cov_w * (1.0 + 0.5 * specificity);
    let cov_cap = cov_norm.min(0.4);
    let name_cap = name_norm.min(0.5);
    bm25_w * c.bm25_score + cov_w * cov_cap + config.name_w * name_cap
} else {
    // walk discoveries already weight coverage heavily
    1.5 * cov_norm + 2.0 * name_norm + config.walk_weight * walk_norm
};
```

The adjustment is bounded: BM25 weight varies from 4.0 (broad) to 2.8 (specific).
Coverage weight varies from 1.0 (broad) to 1.5 (specific). Additive, not replacement.

### Verification

- Specific queries ("encode VLQ source map") should see improved ranking
- Broad queries ("handle request") should be unchanged (BM25 already right)
- No regression on any codebase

## Phase 3: Per-Family Signal Gating

### What

Wire `RetrievalPolicy` into `ScoreConfig` so each query family gets its own
scoring parameters. Currently `RetrievalPolicy` has `bm25_lock_strength`,
`diversity_boost`, and `evidence_weight` but they're unused in the v2 pipeline.

### Why

The holographic research's most important lesson: different query types need
different signal combinations. SymbolExact should rely entirely on BM25 + name
lookup. NaturalAbstract should lean into structural expansion. CrossCuttingSet
should maximize diversity. The family classifier already exists; the scoring
pipeline just doesn't use its output.

### Changes

#### scoring.rs

Extend `ScoreConfig` to carry family-derived parameters:
```rust
pub struct ScoreConfig {
    pub bm25_w: f64,
    pub cov_w: f64,
    pub name_w: f64,
    pub walk_weight: f64,
    pub name_overlap_gate: f64,
    pub name_overlap_max_w: f64,
    pub diversity_max_per_file: usize,
    pub walk_enabled: bool,
    pub name_overlap_enabled: bool,
}

impl ScoreConfig {
    pub fn for_family(family: QueryFamily) -> Self {
        match family {
            QueryFamily::SymbolExact => Self {
                bm25_w: 5.0,
                cov_w: 0.8,
                name_w: 1.0,
                walk_weight: 0.5,
                name_overlap_gate: 0.4,
                name_overlap_max_w: 1.5,
                diversity_max_per_file: 3,
                walk_enabled: false,
                name_overlap_enabled: true,
            },
            QueryFamily::SymbolPartial => Self {
                bm25_w: 4.5,
                cov_w: 1.0,
                name_w: 1.2,
                walk_weight: 0.8,
                name_overlap_gate: 0.3,
                name_overlap_max_w: 1.8,
                diversity_max_per_file: 3,
                walk_enabled: true,
                name_overlap_enabled: true,
            },
            QueryFamily::FilePath => Self {
                bm25_w: 3.0,
                cov_w: 1.5,
                name_w: 0.8,
                walk_weight: 0.3,
                name_overlap_gate: 0.5,
                name_overlap_max_w: 1.0,
                diversity_max_per_file: 5,
                walk_enabled: false,
                name_overlap_enabled: false,
            },
            QueryFamily::ErrorDebug => Self {
                bm25_w: 3.5,
                cov_w: 1.5,
                name_w: 1.5,
                walk_weight: 1.2,
                name_overlap_gate: 0.25,
                name_overlap_max_w: 2.0,
                diversity_max_per_file: 3,
                walk_enabled: true,
                name_overlap_enabled: true,
            },
            QueryFamily::NaturalDescriptive => Self {
                bm25_w: 3.0,
                cov_w: 1.5,
                name_w: 2.0,
                walk_weight: 1.0,
                name_overlap_gate: 0.25,
                name_overlap_max_w: 2.0,
                diversity_max_per_file: 3,
                walk_enabled: true,
                name_overlap_enabled: true,
            },
            QueryFamily::NaturalAbstract => Self {
                bm25_w: 2.5,
                cov_w: 2.0,
                name_w: 1.5,
                walk_weight: 1.5,
                name_overlap_gate: 0.2,
                name_overlap_max_w: 2.0,
                diversity_max_per_file: 2,
                walk_enabled: true,
                name_overlap_enabled: true,
            },
            QueryFamily::CrossCuttingSet => Self {
                bm25_w: 2.0,
                cov_w: 2.0,
                name_w: 1.0,
                walk_weight: 1.5,
                name_overlap_gate: 0.3,
                name_overlap_max_w: 1.5,
                diversity_max_per_file: 1,
                walk_enabled: true,
                name_overlap_enabled: false,
            },
            QueryFamily::Relationship => Self {
                bm25_w: 3.0,
                cov_w: 1.5,
                name_w: 1.0,
                walk_weight: 2.0,
                name_overlap_gate: 0.3,
                name_overlap_max_w: 1.5,
                diversity_max_per_file: 3,
                walk_enabled: true,
                name_overlap_enabled: false,
            },
        }
    }
}
```

#### pipeline.rs

Use family-derived config:
```rust
let score_config = ScoreConfig::for_family(family);
// Remove the hardcoded ScoreConfig construction

// Gate graph walk:
if score_config.walk_enabled {
    // ... existing BFS walk code ...
}

// Use diversity_max_per_file instead of hardcoded 3
```

Pass `family: QueryFamily` into `unified_search()`.

#### search.rs

Pass family to pipeline:
```rust
let raw_results = crate::pipeline::unified_search(
    &query.query,
    ci,
    &seeds,
    &pipeline_config,
    family,  // NEW
);
```

### Verification

- SymbolExact: should be unchanged or slightly improved (higher BM25 weight)
- NaturalAbstract: should improve (lower BM25 lock, higher walk weight)
- CrossCuttingSet: diversity_max_per_file=1 forces diverse results
- ErrorDebug: walk_weight=1.2 gives more structural expansion

## Phase 4: Neighborhood Term Fingerprints

### What

For each symbol, pre-compute the union of decomposed terms from its direct 1-hop
neighbors (calls, imports, contains). Store as a compressed term set in CruncherIndex.
At query time, compute overlap between query terms and neighborhood terms as a
disambiguation signal.

### Why

The holographic research's Phase 11A (predictive surprise) showed that the 1-hop
neighborhood carries strong disambiguation signal. Generic names like `run` have
very different neighborhoods depending on context — `run` in a scheduler vs `run`
in a test harness. The neighborhood terms capture this without needing embeddings
or FFT vectors.

The v1 predictive model was 10.5GB RAM. This approach is ~200 bytes per symbol
(average ~20 unique terms × 10 bytes each) — about 4MB for 20K symbols. Zero
new dependencies.

### Changes

#### cruncher.rs

Add `neighbor_terms` to `CruncherIndex`:
```rust
pub struct CruncherIndex {
    // ... existing fields ...
    pub neighbor_terms: Vec<HashSet<String>>,  // 1-hop neighbor term union
}
```

In `build_cruncher_index()`, after building adjacency lists:
```rust
eprintln!("  Cruncher: building neighbor term fingerprints...");
let neighbor_terms: Vec<HashSet<String>> = (0..n)
    .map(|i| {
        let mut terms = HashSet::new();
        for edge in outgoing[i].iter().take(30) {
            for t in term_sets[edge.target].terms.keys().take(10) {
                terms.insert(t.clone());
            }
        }
        for edge in incoming[i].iter().take(30) {
            for t in term_sets[edge.target].terms.keys().take(10) {
                terms.insert(t.clone());
            }
        }
        terms
    })
    .collect();
```

Add `neighbor_match_score()` function:
```rust
pub fn neighbor_match_score(
    query_terms: &[QueryTerm],
    neighbor_terms: &HashSet<String>,
) -> f64 {
    let mut score = 0.0f64;
    let mut matched = 0usize;
    for qt in query_terms {
        for variant in &qt.variants {
            if neighbor_terms.contains(variant) {
                score += qt.idf;
                matched += 1;
                break;
            }
            for nt in neighbor_terms {
                if nt.contains(variant) || variant.contains(nt.as_str()) {
                    score += qt.idf * 0.5;
                    matched += 1;
                    break;
                }
            }
        }
    }
    score
}
```

#### scoring.rs

Add `neighbor_score` to `Candidate`:
```rust
pub struct Candidate {
    // ... existing fields ...
    pub neighbor_score: f64,
}
```

In `score_candidates()`, add a gated neighbor boost:
```rust
let neighbor_boost = if c.neighbor_score > 0.0 && config.walk_enabled {
    let neighbor_norm = c.neighbor_score / idf_sum.max(1e-10);
    if neighbor_norm > 0.1 {
        0.5 * neighbor_norm
    } else {
        0.0
    }
} else {
    0.0
};

let raw = (base + name_overlap_additive + neighbor_boost)
    * coverage_frac.powf(0.3)
    * seed_bonus
    * kb
    * tp;
```

Only active for families with `walk_enabled` — symbol lookups don't need
neighborhood context.

#### pipeline.rs

Compute `neighbor_score` when populating candidates:
```rust
let ns = neighbor_match_score(&query_terms, &idx.neighbor_terms[i]);

candidates.insert(i, Candidate {
    // ... existing fields ...
    neighbor_score: ns,
});
```

### Verification

- tokio should improve: generic names get disambiguated by their neighborhoods
- esbuild should be neutral: descriptive names already have good BM25 signal
- signetai should be neutral or slightly positive
- Memory increase should be ~4MB for 20K symbols (negligible)

## Phase 5: ErrorDebug Source Pattern Matching

### What

For ErrorDebug queries, scan source code for quoted strings and error patterns
in the query. Any symbol whose source contains the error string becomes a seed
candidate, bypassing BM25 entirely.

### Why

The v1 research (Phase 26) showed source scan seeds helped ErrorDebug queries
specifically. Grep still outperforms graph-based retrieval for finding error
messages because error strings are literal text, not decomposed identifiers.
This is the one case where substring matching beats term matching.

Gated to ErrorDebug queries only (as v1 research mandated) to prevent false
positives on other query types.

### Changes

#### seeds.rs

Add source scan seed generator:
```rust
pub fn source_scan_seeds(
    db: &GraphDb,
    query: &str,
    existing_seeds: &[(i64, f64)],
) -> Vec<(i64, f64)> {
    let patterns: Vec<String> = extract_quoted_and_error_strings(query);
    if patterns.is_empty() {
        return Vec::new();
    }

    let existing_ids: HashSet<i64> = existing_seeds.iter().map(|(id, _)| *id).collect();
    let conn = db.conn();
    let mut candidates: HashMap<i64, f64> = HashMap::new();

    for pattern in &patterns {
        let sql = "SELECT id FROM symbols WHERE source LIKE ?1 LIMIT 50";
        if let Ok(mut stmt) = conn.prepare(sql) {
            let pat = format!("%{}%", pattern.replace('%', "\\%").replace('_', "\\_"));
            if let Ok(rows) = stmt.query_map([&pat], |row| row.get::<_, i64>(0)) {
                for id in rows.filter_map(|r| r.ok()) {
                    if !existing_ids.contains(&id) {
                        *candidates.entry(id).or_insert(0.0) += 1.0;
                    }
                }
            }
        }
    }

    candidates.into_iter().collect()
}

fn extract_quoted_and_error_strings(query: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    
    // Extract quoted strings
    for cap in regex::Regex::new(r#""([^"]{3,})""#).unwrap().captures_iter(query) {
        patterns.push(cap[1].to_string());
    }
    for cap in regex::Regex::new(r"'([^']{3,})'").unwrap().captures_iter(query) {
        patterns.push(cap[1].to_string());
    }
    
    // Extract error-specific phrases (consecutive words near error signals)
    let lower = query.to_lowercase();
    for sig in ERROR_SIGNALS {
        if lower.contains(sig) {
            // Take the words around the error signal
            let words: Vec<&str> = query.split_whitespace().collect();
            for (i, w) in words.iter().enumerate() {
                if w.to_lowercase().contains(sig) && words.len() > 1 {
                    // Grab the phrase: 2 words before and after
                    let start = i.saturating_sub(2);
                    let end = (i + 3).min(words.len());
                    let phrase: Vec<&str> = words[start..end].to_vec();
                    let phrase_str = phrase.join(" ");
                    if phrase_str.len() >= 4 {
                        patterns.push(phrase_str);
                    }
                }
            }
        }
    }
    
    patterns.dedup();
    patterns
}
```

Wire into `SeedConfig`:
```rust
impl SeedConfig {
    pub fn for_family(family: QueryFamily) -> Self {
        Self {
            family,
            allow_per_term: is_nl,
            allow_graph: is_nl,
            allow_numeric: is_nl,
            allow_source_scan: family == QueryFamily::ErrorDebug,  // NEW
        }
    }
}
```

### Verification

- ErrorDebug category should improve significantly (currently 0.000-0.171 NDCG)
- No regression on other categories (source scan is gated to ErrorDebug only)
- No measurable speed impact (ErrorDebug is a minority of queries)

## Phase 6: Integration and Benchmarking

### What

After all phases are implemented and individually verified:

1. Run full benchmark suite on all 3 codebases
2. Compare against Phase 0 baseline
3. Tune per-family ScoreConfig parameters if needed
4. Update docs/research.md with Phase 29 (v3)

### Success Criteria

| Metric | v2 Baseline | v3 Target | Reasoning |
|--------|------------|-----------|-----------|
| signetai NDCG | 0.330 | >= 0.330 | No regression (already good) |
| signetai MRR | 0.900 | >= 0.900 | No regression |
| esbuild NDCG | 0.405 | >= 0.405 | No regression |
| esbuild MRR | 0.940 | >= 0.940 | No regression |
| tokio NDCG | 0.221 | >= 0.270 | Recover ~70% of v1 regression |
| tokio MRR | 0.848 | >= 0.920 | Recover ~60% of v1 regression |
| ErrorDebug NDCG | ~0.0-0.17 | >= 0.20 | Source scan + neighbor terms |
| NL-abstract NDCG | ~0.05-0.27 | >= 0.30 | Per-family config + neighbor boost |

### Non-Goals

- No new dependencies
- No new disk artifacts beyond CruncherIndex extension (~4MB)
- No spectral diffusion, no FFT, no predictive models
- No neural embeddings

## Implementation Order

Each phase must compile, pass all existing tests, and benchmark before the next
phase begins. If a phase causes regression, tune or revert that phase before
proceeding.

1. Phase 0: Baseline capture (no code changes)
2. Phase 1: Gated name overlap scoring (scoring.rs + cruncher.rs + pipeline.rs)
3. Phase 2: Specificity-weighted coverage (scoring.rs only)
4. Phase 3: Per-family signal gating (scoring.rs + pipeline.rs + search.rs)
5. Phase 4: Neighborhood term fingerprints (cruncher.rs + scoring.rs + pipeline.rs)
6. Phase 5: ErrorDebug source pattern matching (seeds.rs only)
7. Phase 6: Full benchmark + docs update

## Memory Budget

| Component | v2 Size | v3 Size | Delta |
|-----------|---------|---------|-------|
| CruncherIndex (cruncher.bin.zst) | 6.5MB | ~10MB | +3.5MB (neighbor_terms) |
| HoloIndex | N/A | N/A | 0 (not added) |
| SpectralIndex | N/A | N/A | 0 (not added) |
| PredictiveModel | N/A | N/A | 0 (not added) |
| **Total** | **6.5MB** | **~10MB** | **+3.5MB** |

Compare to v1's 75MB cache + 18GB RAM. v3 adds 3.5MB at index time, zero at
query time beyond what's already in the CruncherIndex.

## Files Modified

| File | Phase | Change |
|------|-------|--------|
| scoring.rs | 1, 2, 3, 4 | Candidate fields, ScoreConfig::for_family, gated boosts |
| pipeline.rs | 1, 3, 4 | family param, name_overlap computation, neighbor scoring |
| cruncher.rs | 1, 4 | compute_name_overlap, neighbor_terms, neighbor_match_score |
| search.rs | 3 | Pass family to unified_search |
| seeds.rs | 5 | source_scan_seeds, allow_source_scan in SeedConfig |
