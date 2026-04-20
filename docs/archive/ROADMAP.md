# GraphIQ Roadmap

## The Claim

GraphIQ beats grep. Not by replacing lexical search, but by preserving its strengths and adding structure where structure actually helps.

Two dimensions:

**1. Retrieval quality.** Can the system find the right answer, ranked higher, more often?

| | GraphIQ | Grep |
|---|---|---|
| NDCG@10 signetai | **0.399** | 0.343 |
| NDCG@10 esbuild | **0.420** | 0.277 |
| MRR@10 signetai | **0.393** | 0.154 |
| MRR@10 tokio | **0.717** | 0.317 |
| MRR@10 esbuild | **0.368** | 0.185 |

First-hit retrieval isn't close. GraphIQ wins MRR by 2-2.6x across all three codebases.

**2. Quality of what surfaces.** Grep returns symbol names. GraphIQ returns ranked symbols with structural context — what calls them, what they call, where they sit in the architecture. The agent doesn't just get a name. It gets a fact about the codebase, weighted by relevance.

This second dimension is the real product. The first dimension proves the foundation works. The second dimension is what makes someone choose this over grep.

---

## Where It Breaks Down

The claim holds but isn't uniform:

- **tokio NDCG**: 0.179 vs 0.322. Generic function names (`run`, `handle`, `poll`) make structural signals unreliable. Grep's raw substring matching is harder to beat when names carry no signal.
- **tokio nl-abstract**: 0.000 vs 0.585. "What mechanism handles cooperative task yielding" — Deformed mode produces nothing. The spectral index doesn't capture tokio's structural patterns.
- **symbol-exact**: Should tie grep but doesn't always. `spawn_blocking` gets 0.542 vs 1.000. GooberV5 routing for exact lookups has room to improve.

These aren't proof the approach fails. They're proof the approach has edges, and tokio is the hardest edge.

---

## What To Do

### Tier 1: Make the claim airtight

The claim is "GraphIQ beats grep for agent recall." Make it true everywhere, not just most places.

**1a. Fix symbol-exact routing**

GooberV5 should match or beat grep on exact symbol lookups. Currently it doesn't — `spawn_blocking` is 0.542 vs grep's 1.000. The issue is that GooberV5's holographic name gate can demote exact matches when BM25 scoring is weak.

Action: When the classifier says SymbolExact, bypass the full pipeline and do a direct name lookup with BM25 fallback. An exact match should never score below 0.9.

**1b. Close the tokio gap**

tokio NDCG (0.179 vs 0.322) is the one place grep clearly wins. The breakdown:
- nl-abstract: 0.000 vs 0.585 — Deformed produces nothing
- error-debug: 0.000 vs 0.494 — same
- symbol-partial: 0.038 vs 0.235 — GooberV5 can't disambiguate generic names

Action: Diagnose why Deformed mode fails on tokio. Is the spectral index degenerate for tokio's graph structure? Is heat diffusion propagating noise instead of signal? Fix the underlying method, don't add heuristics on top.

**1c. Benchmark CI**

Run NDCG and MRR on every push. No regressions without explicit acknowledgment. This prevents the historical pattern of improving one codebase while regressing another.

### Tier 2: Make what surfaces actually good

This is the product differentiator. Grep gives you a name. GraphIQ should give you a fact.

**2a. Structural context in search results**

Every search result should carry its structural neighborhood. Not as a separate tool call, but inline:

```
search("rate limit middleware") →
  1. RateLimiter (struct, score: 0.94)
     calls: checkLimit(), TokenBucket.consume()
     called by: setupMiddleware(), chain.execute()
     constants: MAX_REQUESTS (shared with RateLimitError, loadConfig)
```

This is what makes the result a *fact about the codebase* rather than just a symbol name. The agent doesn't need a follow-up blast call. The answer is already there.

**2b. Importance ranking**

Not all symbols are equal. An entry point is more important than a leaf. A boundary symbol is more important than an internal helper. Rank results by structural importance, not just text relevance.

A query like "how does auth work" should surface `authenticate()` before `hashPassword()` not because of text matching, but because `authenticate()` is the entry point — it's the answer to the question "how does auth work" in a way `hashPassword()` isn't.

**2c. Numeric bridges as first-class signals**

Numbers are structural glue. `429`, `404`, `200`, `3850`, `0.5` — these connect symbols across modules that named edges miss. Wire the existing numeric bridge extraction into the spectral diffusion pipeline. This helps nl-abstract and error-debug queries: "what port does the server listen on" should find `DEFAULT_PORT = 3850`.

### Tier 3: Clean house

**3a. Remove dead code**

~7K lines of abandoned approaches (af26, afmo, hrr, hrr_v2, qhrr, windtunnel, hypergraph, motifs). These are research artifacts, not production code. They add noise to the codebase and confuse anyone trying to understand the system.

**3b. Expand codebase coverage**

Three codebases is a thin claim. Add Python (Django/Flask) and Java (Spring). If the approach is principled, it should work on naming regimes beyond TypeScript, Rust, and Go.

**3c. Latency**

Graph is built at query time. Current cold latency is the full pipeline. Target: <50ms cold, <10ms cached. This matters because the agent comparison isn't just "is the result better?" — it's "is the result better *fast enough* that I don't regret not just grepping?"

---

## Success Criteria

### Tier 1 — retrieval must beat grep everywhere

| Metric | Current | Target |
|---|---|---|
| NDCG signetai | 0.399 | >= 0.420 |
| NDCG esbuild | 0.420 | >= 0.440 |
| NDCG tokio | 0.179 | >= 0.340 (match grep) |
| MRR signetai | 0.393 | >= 0.420 |
| MRR tokio | 0.717 | >= 0.720 (maintain) |
| MRR esbuild | 0.368 | >= 0.400 |
| symbol-exact NDCG | 0.49-1.00 | >= 0.95 everywhere |

### Tier 2 — what surfaces must be better than a name

| Metric | Target |
|---|---|
| Search results carry callers/callees | Inline, not separate tool |
| Importance-weighted ranking | Top-3 includes highest-importance relevant symbol |
| Numeric bridge queries | "what port" / "what timeout" find the constant |

### Tier 3 — production readiness

| Metric | Target |
|---|---|
| Dead code removed | ~7K lines gone |
| Codebases benchmarked | 5 (add Python, Java) |
| Cold query latency | <50ms |
| Benchmark CI | Every push |

---

## The Frame

GraphIQ is better than grep alone for agent recall. The remaining work is making that advantage robust across harder query families and harder naming regimes, and making what surfaces feel like facts about the codebase rather than just matches.

Grep is the real enemy. Beating a dumb but brutally effective tool is harder than beating some fluffy embedding baseline nobody actually trusts. The results show it's working. The job is to make it undeniable.
