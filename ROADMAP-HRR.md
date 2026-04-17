# HRR Roadmap: Closing the Gap

Current state: HRR-Rerank = 1.795 aggregate (+0.014 over BM25). HRR-Pure = 1.509 (85% of BM25 with zero text search).

## The Big Opportunity

HRR-Pure gets 0.632/0.463/0.414 without ANY text search. If we can close the gap between HRR-Pure and BM25, we have a retrieval system that understands CODE STRUCTURE, not just text matching. The pure holographic search is the real prize.

## Current Architecture

```
Symbol identity = normalized sum of random term vectors (from symbol name)
Hologram = identity + Σ_{outgoing} weight × relation ⊛ neighbor_identity
                   + Σ_{incoming} weight × relation_inv ⊛ neighbor_identity
Query = normalized sum of term vectors (from query text)
Match = dot(query, hologram)
```

1024 dimensions, FFT-based circular convolution, deterministic seeded random vectors.

## Avenues to Explore

### 1. Enrich Identity Vectors (HIGH IMPACT)

**Problem:** Identity is built from symbol name terms only. A symbol like `parse_edge_kind_json` gets terms ["parse", "edge", "kind", "json"] but its qualified name `graphiq_core::db::parse_edge_kind_json` also contains "graphiq", "core", "db" — context that's currently lost.

**Idea:** Include ALL terms from the symbol's context:
- Qualified name components (namespace, module path)
- File path terms (directory names = domain context)
- Symbol kind as a binding: `is_function ⊛ identity`
- Terms from the symbol's doc comments (if parsed)

**Why:** The query "database insert" should match `db::insert_symbol` even if the symbol name is just "insert_symbol". The file path "db.rs" provides the "database" signal that's currently invisible to HRR.

**Implementation:** Modify `compute_hrr` to build identity from enriched term set.

### 2. Multi-Hop Bindings (HIGH IMPACT)

**Problem:** Current holograms encode 1-hop relationships only. A→B→C means A's hologram knows about B, and B's knows about C, but A doesn't know about C.

**Idea:** Add 2-hop bindings with decay:
```
hologram[i] += decay × relation_a ⊛ relation_b ⊛ neighbor_2hop_identity
```

Circular convolution is associative: `(r1 ⊛ r2) ⊛ id = r1 ⊛ (r2 ⊛ id)`. So `relation_a ⊛ relation_b` produces a new vector that encodes the composite relation "a then b". This is a NEW relation type discovered through binding composition.

**Risk:** Noise amplification in dense graphs (same issue as propagator). Mitigate with:
- Only add 2-hop for strong edge types (Calls, Contains)
- Decay factor (0.3-0.5)
- Skip intermediaries with high degree (>20 neighbors)

### 3. Query-Side Structural Expansion (MEDIUM-HIGH IMPACT)

**Problem:** Query vector is just a bag of terms. A query like "what calls parse_edge" should encode the structural intent "I want symbols that CALL something related to parse_edge."

**Idea:** Parse query structure:
- "calls X" → query = `calls_vec ⊛ X_identity`
- "contains X" → query = `contains_vec ⊛ X_identity`
- "implemented by X" → query = `implements_vec ⊛ X_identity`
- Default: plain term superposition (current approach)

**Why:** Structural queries are HRR's superpower. Unbinding via circular correlation lets us ask "what is bound to X through relation R?" — this is impossible with BM25.

**Implementation:** Detect structural patterns in query text, encode as bindings instead of plain superposition.

### 4. IDF-Weighted Term Vectors (MEDIUM IMPACT)

**Problem:** All term vectors contribute equally to identity. Common terms like "get", "set", "new" dominate and create spurious matches.

**Idea:** Scale each term's contribution by its IDF:
```
identity = Σ_{terms t in name} idf(t) × term_vec(t)
```

This naturally downweights common terms and upweights distinctive ones. Same principle as BM25's IDF component, but applied in the holographic space.

### 5. Bidirectional Binding with Role-Filler Pairs (MEDIUM IMPACT)

**Problem:** Current encoding doesn't distinguish role from filler. `calls ⊛ target` loses the directionality — it's just a binding.

**Idea:** Use Plate's full role-filler encoding:
```
hologram[i] = identity(i) + Σ (role ⊛ filler)
where role = relation_vec ⊛ source_identity
      filler = target_identity
```

This makes each binding carry information about BOTH the relation type AND the source symbol. When querying with `role ⊛ target`, we can recover the source via circular correlation.

**Why:** Currently, two symbols that call the same function get similar holograms even if they're unrelated. Role-filler encoding preserves more structural information.

### 6. Hybrid HRR + AFMO Rerank (MEDIUM IMPACT)

**Problem:** HRR+AFMO combined regressed (1.786 vs HRR alone 1.795). The multiplicative boosts over-amplify.

**Idea:** Instead of stacking rerankers multiplicatively:
- Use HRR as the PRIMARY structural signal (replaces graph_boost in AFMO)
- Use AFMO's hyperbolic similarity as a SECONDARY depth signal
- Or: use HRR similarity to SELECT which AFMO boost level to apply (gate-based, not multiplicative)

**Implementation:** Modify `afmo_rerank` to accept HRR similarity as an input, use it to gate the hyp_boost thresholds.

### 7. Heat Kernel Diffusion (DEFERRED — after HRR is maxed)

Build the graph Laplacian L, compute heat kernel K_t(u,v) = Σ_k e^{-tλ_k} φ_k(u)φ_k(v). Use as an alternative structural proximity measure. Time parameter t controls locality. Could replace or complement HRR's graph encoding.

## Baselines to Beat

| Method | Self | Tokio | Signetai | Aggregate |
|---|---|---|---|---|
| BM25 | 0.715 | 0.539 | 0.527 | 1.781 |
| AFMO-Rerank | 0.705 | 0.538 | 0.533 | 1.776 |
| **HRR-Rerank** | **0.719** | **0.546** | **0.530** | **1.795** |
| HRR-Pure | 0.632 | 0.463 | 0.414 | 1.509 |

Target: HRR-Rerank aggregate ≥ 1.810, HRR-Pure ≥ 1.600

## Recommended Order for Tomorrow

1. **Enriched identity** (file path + qualified name terms) — easiest win
2. **IDF-weighted term vectors** — straightforward, high expected value
3. **Benchmark** — see if enriched identity moves HRR-Pure
4. **Multi-hop bindings** — if single-hop is saturated
5. **Structural query parsing** — unlock HRR's binding queries
6. **Heat Kernel** — if HRR plateau is hit

## Files

- `crates/graphiq-core/src/hrr.rs` — HRR module (FFT, circular convolution, index build, search, rerank)
- `crates/graphiq-core/src/afmo.rs` — AFMO module (Poincaré ball + bandpass code, partially used)
- `crates/graphiq-bench/src/main.rs` — Bench with HRR-Pure, HRR-Rerank, HRR+AFMO evaluations
- `ROADMAP-AFMO-V2.md` — Previous roadmap (propagator/bandpass ideas, mostly dead)
- `ROADMAP-PHASE6.md` — Phase 6 overview
