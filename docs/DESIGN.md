# GraphIQ — Code Intelligence System

> Drop a codebase in. Get instant, accurate retrieval. Structural indexes bridge the semantic gap. Embeddings rerank only when needed.

## Why This Works (and sqmd Didn't)

sqmd's fatal mistake: **inverted funnel**. It embedded the entire corpus first, then tried to use FTS as a noisy fallback. The retrieval pipeline was 6 layers of increasingly expensive operations applied to 20,000 chunks, when BM25 on identifiers alone would have caught most of the right hits in the top 200.

GraphIQ inverts the inversion. The insight is simple: **code identifiers carry meaning**. `RateLimiter`, `rate_limit.ts`, `authenticateUser`, `RateLimitError` — these are semantically rich tokens that FTS handles natively. The "semantic gap" people try to close with embeddings is mostly solvable with structural indexes (call graphs, import graphs, type hierarchies) at zero embedding cost.

**The funnel:**

```
Query: "rate limit middleware"
        │
        ├─ Hot Context Cache hit? → return instantly (< 1ms)
        │  (exact query match or fuzzy overlap with cached result set)
        │
        ▼
┌─────────────────────┐
│  Layer 1: BM25/FTS  │  ~5ms   → 200 candidates
│  Identifier-aware   │  rateLimit, rate_limit, middleware all match
└────────┬────────────┘
         │
         ▼
┌─────────────────────────────┐
│  Layer 2: Structural Expand │  ~10-20ms  → ~500 candidates
│  Import graph  → callers    │  api/server.ts imports rateLimitMiddleware
│  Call graph    → callees    │  TokenBucket used by RateLimiter
│  Type hierarchy → impls     │  RateLimiter implements IMiddleware
│  Test association           │  rateLimit.test.ts covers RateLimiter
│  (uses cached neighborhoods │
│   for hot symbols)          │
└────────┬────────────────────┘
         │
         ▼
┌──────────────────────────────────┐
│  Layer 3: Cheap Rerank           │  ~5ms   → top 50
│  Path weights per edge type      │  Calls=1.0, Contains=0.9, Imports=0.6
│  Heuristics (signal density,     │  penalize 500-line functions
│    entry-point bias, export       │  boost main/index/server files
│    bias, test proximity)          │  boost exported symbols
│  Diversity dampen                 │
└────────┬─────────────────────────┘
         │
         ▼
┌──────────────────────────────────┐
│  Layer 4: Embed Rerank (OPTIONAL)│  ~30ms  → top_k
│  Only on narrowed top 50          │  embed name+signature only
│  Only for natural language queries│  NOT source body
│  Query embedding cached/session   │
│  Blend: heuristic×0.7 + cosim×0.3 │
└──────────────────────────────────┘
         │
         ▼
    Result → feed hot cache
```

By Layer 2, you have 500 candidates structurally connected to your query. Layer 3's cheap rerank (path weights + heuristics, zero embedding cost) cuts to 50. If embeddings are enabled, they only rerank those 50 — never the full corpus. The hot cache short-circuits the whole pipeline for repeated or overlapping queries.

---

## Architecture

### Crate Structure

```
graphiq/
├── Cargo.toml
├── crates/
│   ├── graphiq-core/          # Core library — indexing, storage, retrieval
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── symbol.rs       # Symbol, File, SymbolKind, Visibility
│   │   │   ├── edge.rs         # Edge, EdgeKind, BlastRadius
│   │   │   ├── db.rs           # SQLite schema, migrations, all queries
│   │   │   ├── fts.rs          # BM25 retrieval, identifier tokenization
│   │   │   ├── graph.rs        # Graph traversal, structural expansion
│   │   │   ├── blast.rs        # Blast radius computation (forward/backward)
│   │   │   ├── rerank.rs       # Cheap rerank: path weights, heuristics, embed blend
│   │   │   ├── cache.rs        # Hot context cache — LRU neighborhoods, result cache, source cache
│   │   │   ├── cache.rs        # Hot context cache — LRU neighborhoods, result cache
│   │   │   ├── search.rs       # Unified search API — the funnel
│   │   │   ├── index.rs        # Indexing pipeline orchestrator
│   │   │   ├── tokenize.rs     # Identifier decomposition (camelCase, snake_case)
│   │   │   ├── chunker.rs      # LanguageChunker trait (from sqmd, cleaned)
│   │   │   ├── calls.rs        # AST call site extraction (from sqmd)
│   │   │   ├── languages/      # TreeSitter language parsers (from sqmd)
│   │   │   │   ├── mod.rs
│   │   │   │   ├── typescript.rs
│   │   │   │   ├── rust.rs
│   │   │   │   ├── python.rs
│   │   │   │   ├── go.rs
│   │   │   │   ├── java.rs
│   │   │   │   ├── c.rs
│   │   │   │   ├── cpp.rs
│   │   │   │   ├── ruby.rs
│   │   │   │   ├── yaml.rs
│   │   │   │   ├── json.rs
│   │   │   │   ├── toml.rs
│   │   │   │   ├── html.rs
│   │   │   │   └── css.rs
│   │   │   └── files.rs        # Language detection, project walking
│   │   └── Cargo.toml
│   ├── graphiq-cli/            # CLI — index, search, blast, status, demo, setup
│   │   ├── src/main.rs
│   │   └── Cargo.toml
│   ├── graphiq-mcp/            # MCP server — JSON-RPC 2.0 over stdio
│   │   ├── src/main.rs
│   │   └── Cargo.toml
│   └── graphiq-bench/          # Benchmarking — MRR, Hit@K, latency
│       ├── src/main.rs
│       ├── Cargo.toml
│       └── queries/            # Benchmark query sets
├── .github/workflows/
│   └── release.yml             # CI: build releases on tag push
└── README.md
```

### Salvaged from sqmd

| Component | Files | Status |
|-----------|-------|--------|
| `LanguageChunker` trait | `chunker.rs` | Clean extraction — trait + `make_chunk()` factory |
| TreeSitter language parsers | `languages/*.rs` (17 grammars) | Direct port with cleanup — remove chunk-specific logic, keep AST walking |
| Call site extractor | `call_extractor.rs` | Direct port — `CallSite` struct, per-language AST walking |
| Language detection | `files.rs` | Direct port — extension mapping, `.gitignore` walking |
| `Chunk` → `Symbol` adaptation | `chunk.rs` | Rename to `Symbol`, drop `ChunkType` → use `SymbolKind`, add `qualified_name` |

**Not salvaged**: SQLite schema (rewritten), embedding pipeline (gone), search pipeline (rewritten), enrichment/hints (replaced by structural expansion), communities (replaced by graph traversal), MCP server (rewrite later).

---

## Data Model

### Core Types

```rust
#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub kind: SymbolKind,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: Option<String>,
    pub visibility: Visibility,
    pub doc_comment: Option<String>,
    pub source: String,
    pub content_hash: [u8; 32],
    pub language: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function, Method, Constructor, Destructor,
    Class, Interface, Struct, Enum, EnumVariant,
    Trait, TypeAlias, Module, Namespace,
    Constant, Field, Property,
    Macro,
    Import, Export,
    Section,  // unclaimed code gaps
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public, Private, Protected, Package, Anonymous,
}

#[derive(Debug, Clone)]
pub struct File {
    pub id: i64,
    pub path: String,
    pub language: String,
    pub content_hash: [u8; 32],
    pub mtime_ms: i64,
    pub line_count: u32,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub kind: EdgeKind,
    pub weight: f64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Imports,       // A imports B
    Calls,         // A calls B
    Contains,      // A contains B (class→method, module→child)
    Extends,       // A extends/inherits B
    Implements,    // A implements interface B
    Overrides,     // A overrides method B
    References,    // A references type/symbol B (type annotation, field access)
    Tests,         // A tests B (test file → source symbol)
    ReExports,     // A re-exports B
}
```

### SQLite Schema

```sql
-- Schema v1. Clean. No migration hell.

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO meta (key, value) VALUES ('schema_version', '1');

-- Files
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    language TEXT NOT NULL,
    content_hash BLOB NOT NULL,  -- SHA-256
    mtime_ms INTEGER NOT NULL,
    line_count INTEGER NOT NULL DEFAULT 0
);

-- Symbols (the core unit — every named code entity)
CREATE TABLE symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    qualified_name TEXT,
    kind TEXT NOT NULL,           -- SymbolKind as string
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    signature TEXT,
    visibility TEXT NOT NULL DEFAULT 'public',
    doc_comment TEXT,
    source TEXT NOT NULL,         -- full source text of the symbol
    name_decomposed TEXT NOT NULL, -- camelCase/snake_case → space-separated tokens
    content_hash BLOB NOT NULL,
    language TEXT NOT NULL,
    metadata TEXT DEFAULT '{}',   -- JSON
    importance REAL NOT NULL DEFAULT 0.5  -- structural importance score
);

CREATE INDEX idx_symbols_file ON symbols(file_id);
CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_symbols_kind ON symbols(kind);
CREATE INDEX idx_symbols_qualified ON symbols(qualified_name);

-- FTS5 — identifier-aware full-text search
-- name_decomposed provides camelCase/snake_case tokenization
CREATE VIRTUAL TABLE symbols_fts USING fts5(
    name,
    name_decomposed,
    qualified_name,
    signature,
    source,
    doc_comment,
    file_path,    -- denormalized from files table for FTS matching
    kind,
    language,
    content=symbols,
    content_rowid=id,
    tokenize='porter unicode61'
);

-- Triggers to keep FTS in sync
CREATE TRIGGER symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, name_decomposed, qualified_name, signature, source, doc_comment, file_path, kind, language)
    SELECT new.id, new.name, new.name_decomposed, new.qualified_name, new.signature, new.source, new.doc_comment, f.path, new.kind, new.language
    FROM files f WHERE f.id = new.file_id;
END;

CREATE TRIGGER symbols_ad AFTER DELETE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, name_decomposed, qualified_name, signature, source, doc_comment, file_path, kind, language)
    VALUES ('delete', old.id, old.name, old.name_decomposed, old.qualified_name, old.signature, old.source, old.doc_comment, '', old.kind, old.language);
END;

-- Graph edges (symbol → symbol)
CREATE TABLE edges (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1.0,
    metadata TEXT DEFAULT '{}',
    UNIQUE(source_id, target_id, kind)
);

CREATE INDEX idx_edges_source ON edges(source_id, kind);
CREATE INDEX idx_edges_target ON edges(target_id, kind);
CREATE INDEX idx_edges_kind ON edges(kind);

-- File-level edges (for coarse-grained import/test relationships)
CREATE TABLE file_edges (
    id INTEGER PRIMARY KEY,
    source_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    target_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,  -- 'imports', 'tests', 're_exports'
    metadata TEXT DEFAULT '{}',
    UNIQUE(source_file_id, target_file_id, kind)
);

CREATE INDEX idx_file_edges_source ON file_edges(source_file_id, kind);
CREATE INDEX idx_file_edges_target ON file_edges(target_file_id, kind);

-- Blast radius cache (DISPOSABLE — fully recomputable from edges table)
-- Nothing in the system depends on this being fresh or present.
-- It exists purely as a performance optimization for repeated blast queries.
-- If in doubt, TRUNCATE blast_cache and recompute. It is NOT a source of truth.
-- The in-memory hot cache (cache.rs) is the primary blast cache; this SQLite
-- table is a cold-weather backup for sessions that exceed memory limits.
CREATE TABLE blast_cache (
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    dependent_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    direction TEXT NOT NULL,     -- 'forward' or 'backward'
    distance INTEGER NOT NULL,
    edge_kinds TEXT NOT NULL,    -- JSON array of edge kinds in path
    computed_at INTEGER NOT NULL, -- unix timestamp, for staleness checks
    PRIMARY KEY (symbol_id, dependent_id, direction)
);

CREATE INDEX idx_blast_symbol ON blast_cache(symbol_id, direction);

-- Optional: symbol embeddings (only if embed feature is enabled)
-- Uses sqlite-vec for vector KNN
-- CREATE VIRTUAL TABLE symbol_vec USING vec0(
--     symbol_id INTEGER PRIMARY KEY,
--     embedding float[768]
-- );
```

---

## Indexing Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│                     INDEXING PIPELINE                        │
│                                                             │
│  1. Walk project (respect .gitignore)                       │
│     │                                                       │
│     ▼                                                       │
│  2. Mtime + content-hash prefilter                          │
│     │  Skip unchanged files. Tombstone deleted files.        │
│     ▼                                                       │
│  3. Parallel TreeSitter parse (rayon)                       │
│     │  Per file:                                            │
│     │  ├── Parse AST with language grammar                  │
│     │  ├── Walk declarations → Symbol structs               │
│     │  ├── Fill gaps → Section symbols (max 50 lines)       │
│     │  ├── Extract imports → ImportInfo structs             │
│     │  ├── Extract call sites → CallSite structs            │
│     │  ├── Extract structural rels → extends/implements     │
│     │  └── Decompose identifiers → name_decomposed          │
│     │                                                       │
│     ▼                                                       │
│  4. Serial DB write (transactional)                         │
│     │  ├── Insert/update files                              │
│     │  ├── Insert/update symbols (content-hash dedup)       │
│     │  ├── Insert edges (calls, imports, contains, etc.)    │
│     │  ├── Insert file_edges (file-level imports, tests)    │
│     │  └── FTS auto-updates via triggers                    │
│     │                                                       │
│     ▼                                                       │
│  5. Compute structural importance                           │
│     │  Based on: in-degree (imports+calls), contains count  │
│     │  Updated in-place on symbols.importance               │
│     ▼                                                       │
│  6. (Optional) Embed symbols                               │
│        Only if --embed flag enabled                         │
│        Uses local model (gte-modernbert-base or similar)    │
│        Embeds name_decomposed + signature only (not body)   │
│        Stores in symbol_vec                                 │
└─────────────────────────────────────────────────────────────┘
```

### Incremental Reindexing

- Compare mtime → content_hash for each file
- Unchanged files: skip entirely
- Changed files: re-parse, upsert symbols (content-hash dedup)
- Deleted files: tombstone (set active=0 or delete cascade)
- New files: full parse + insert
- Edges for changed/deleted symbols are rebuilt

### Identifier Decomposition

The `name_decomposed` field is the key to making FTS work for code:

```
authenticateUser  → "authenticate user"
rate_limit_middleware → "rate limit middleware"
HTTPClient        → "http client"
XMLParser         → "xml parser"
```

Algorithm:
1. Split on `_` (snake_case)
2. Split on transitions from lowercase→uppercase (camelCase)
3. Split on transitions from uppercase→uppercase+lowercase (PascalCase abbreviations like `XMLParser` → `XML Parser`)
4. Lowercase all tokens
5. Join with spaces

This means searching "rate limit" will match `RateLimiter`, `rate_limit`, `RATE_LIMIT_CONFIG` — all through FTS without any embedding.

---

## Retrieval: The Funnel

### Layer 1: BM25/FTS (~5ms)

The primary retrieval mechanism. SQLite FTS5 with porter stemming.

**Query construction:**
1. Tokenize user query
2. Decompose each token (apply same camelCase/snake_case splitting)
3. Build FTS query:
   - Try AND first: `"rate" AND "limit" AND "middleware"`
   - If AND returns < 10 results, fall back to OR: `"rate" OR "limit" OR "middleware"`
   - Add wildcard for partial matches on name columns: `"rate*" AND "limit*"`

**FTS column weights** (via `bm25()` column position):
| Column | Weight | Rationale |
|--------|--------|-----------|
| `name` | 10.0 | Exact symbol name is the strongest signal |
| `name_decomposed` | 8.0 | Decomposed identifier tokens |
| `qualified_name` | 6.0 | Full path like `auth.RateLimiter` |
| `signature` | 4.0 | Function signature contains param/return types |
| `doc_comment` | 3.0 | Documentation text |
| `file_path` | 2.0 | File path matching |
| `source` | 1.0 | Body text (lowest — too noisy for high weight) |
| `kind` | 0.5 | Symbol kind (class, function, etc.) |

Returns: top 200 candidates with BM25 scores.

### Layer 2: Structural Expansion (~20ms)

> **graph.rs answers: "what related things help retrieve the answer?"**
> It traverses outbound from FTS hits to find structurally connected symbols that the query might have been *about* but didn't name directly. Its purpose is recall expansion — widening the candidate set intelligently.

For each of the top 20 FTS hits, expand via structural indexes:

```
For each FTS hit H in top 20:
    // Call graph expansion
    callers(H)   → symbols that call H     (edge: Calls, target=H)
    callees(H)   → symbols H calls         (edge: Calls, source=H)
    
    // Import graph expansion
    importers(H) → files that import H's file  (file_edge: imports)
    imported(H)  → files H's file imports       (file_edge: imports)
    
    // Type hierarchy
    implementors(H) → symbols implementing H's interface (edge: Implements)
    parent(H)       → symbols H extends/implements       (edge: Extends/Implements)
    
    // Containment
    members(H)    → symbols contained in H    (edge: Contains, source=H)
    container(H)  → symbol containing H       (edge: Contains, target=H)
    
    // Test association
    tests(H)      → test files/symbols for H  (edge: Tests)
```

Each expanded symbol gets a **decayed score**: `H.fts_score * decay(distance)`

- Direct neighbor: `× 0.5`
- 2 hops: `× 0.25`
- 3 hops: `× 0.1`

Maximum expansion depth: 2 hops (configurable).

Returns: ~500 candidates (FTS hits + structurally expanded).

### Layer 3: Cheap Rerank (~5ms)

The heart of GraphIQ's ranking. Pure computation — no embeddings, no model calls, no I/O beyond what's already in memory. This is what makes the system fast and deterministic.

Cheap rerank has three sub-layers: **path weights**, **heuristics**, and **diversity dampen**. They run in that order, each refining the candidate set.

#### 3a. Path Weights

Not all edges are equal. When structural expansion pulls in candidates via graph traversal, the edge type determines how much signal that connection carries:

| EdgeKind | Path Weight | Rationale |
|----------|-------------|-----------|
| `Calls` | 1.00 | Direct code dependency — strongest signal |
| `Contains` | 0.90 | Class→member: structural ownership |
| `Implements` | 0.80 | Interface→implementation: behavioral contract |
| `Extends` | 0.80 | Inheritance: shared behavior |
| `Overrides` | 0.75 | Overridden method: specialized behavior |
| `Tests` | 0.55 | Test association: verified behavior |
| `Imports` | 0.50 | File-level dependency: weakest (may be unused) |
| `References` | 0.40 | Type annotation/field access: incidental |

When a candidate was reached via structural expansion, its score is scaled by the path weight of the edge that connected it:

```
expansion_score = origin_fts_score * decay(distance) * path_weight(edge_kind)
```

A symbol reached via a `Calls` edge at distance 1 gets: `origin_score × 0.5 × 1.0 = 0.50`
The same symbol reached via a `References` edge at distance 2: `origin_score × 0.25 × 0.4 = 0.10`

**Multi-path boosting**: If a candidate is reachable via multiple paths, take the max score across all paths, then add a small bonus for each additional path: `+0.05 × min(additional_paths, 3)`. This rewards symbols that are structurally connected to multiple FTS hits from different directions.

#### 3b. Heuristics

Deterministic signals computed from symbol metadata. No ML, no embeddings, just fast math.

**Every heuristic is individually toggleable and logged.** When retrieval feels off, you need to know *which* signal helped and which hurt. Each heuristic produces a named multiplier, and in debug mode every result includes the full score breakdown.

```rust
#[derive(Debug, Clone)]
pub struct HeuristicConfig {
    pub density: bool,         // default: true
    pub entry_point: bool,     // default: true
    pub export: bool,          // default: true
    pub test_proximity: bool,  // default: true
    pub importance: bool,      // default: true
    pub recency: bool,         // default: true
}

#[derive(Debug, Clone)]
pub struct ScoreBreakdown {
    pub layer2_score: f64,
    pub heuristics: Vec<(&'static str, f64)>,  // (name, multiplier) per heuristic
    pub heuristic_multiplier: f64,
    pub path_weight: f64,
    pub diversity_dampen: f64,
    pub final_score: f64,
}
```

Debug mode (`--debug` flag or `search(query).with_debug(true)`) attaches `ScoreBreakdown` to every result and prints it:

```
#1  RateLimiter.handle()  score=0.847
    layer2=0.920  path_weight=1.00  density=1.00  entry=1.15  export=1.10
    test_prox=1.10  importance=0.95  recency=0.83  diversity=1.00
    → heuristic_multiplier=1.085  final=0.920×1.085×1.00=0.999→0.847(after dampen)

#2  processRequest()  score=0.312
    layer2=0.780  path_weight=0.40  density=0.16  entry=1.00  export=1.00
    test_prox=1.00  importance=0.70  recency=0.45  diversity=0.85
    → heuristic_multiplier=0.050  final=0.780×0.050×0.40×0.85=0.013→0.312
```

Now you can see that `processRequest` got killed by density (0.16 — it's a 500-line monster) and diversity dampen (0.85 — another result from the same file ranked higher). Actionable.

Individual heuristics:

**Signal density** — penalize bloated symbols, reward focused ones:
```
density = min(1.0, 80 / symbol.line_count)
```
A 20-line function gets 1.0. A 400-line monster gets 0.2. The intuition: a focused function named `handleRateLimit` is a better result than a 500-line `processRequest` that happens to mention rate limiting once.

**Entry-point bias** — boost symbols in conventionally important files:
```
if file_path matches /(main|index|app|server|mod|lib)\.(ts|rs|py|go|java)$/
    entry_boost = 1.15
else
    entry_boost = 1.0
```

**Exported bias** — public API symbols are better answers for most queries:
```
if symbol.visibility == Public && symbol.kind in [Function, Class, Interface, Struct, Enum, Trait, TypeAlias]
    export_boost = 1.1
else
    export_boost = 1.0
```

**Test proximity** — symbols with dedicated tests are more important:
```
if exists edge Tests(source=*, target=symbol)
    test_boost = 1.1
else
    test_boost = 1.0
```

**Importance decay** — use the pre-computed structural importance (based on in-degree), but cap it:
```
importance_factor = 0.5 + (0.5 * min(symbol.importance, 1.0))
```
Range: [0.5, 1.0]. High-importance symbols get 1.0, low-importance get 0.5. Never zero — every symbol stays in consideration.

**Recency** — slight bias toward recently modified files:
```
days_since_mtime = (now - file.mtime_ms) / 86400000
recency = 1.0 / (1.0 + days_since_mtime / 90.0)
```
90-day half-life. A file modified today gets 1.0, 90 days ago gets 0.5, a year ago gets ~0.2. Gentle, not aggressive.

**Combined heuristic score:**
```
heuristic_multiplier = (
    density * entry_boost * export_boost * test_boost * importance_factor * recency
)
final_layer3_score = layer2_score * heuristic_multiplier
```

Multiplicative, not additive — so a strong FTS hit in a 500-line private function with no tests still shows up, just ranked below the clean exported version.

#### 3c. Diversity Dampen

After heuristic scoring, penalize same-file clustering:

```
for each result R sorted by score:
    same_file_count = count(results_before_R with same file_id)
    R.score *= 0.85 ^ same_file_count
```

This ensures the top 10 results span at least 4-5 different files instead of showing 8 methods from one class.

Returns: top 50 candidates, ranked by `layer2_score * heuristic_multiplier * diversity_dampen`.

### Layer 4: Embedding Rerank (optional, ~30ms)

Only activated when all three conditions are met:
1. Embeddings are enabled (`--embed` flag)
2. Query appears to be natural language (heuristic: >3 tokens, no camelCase/snake_case, contains stop words)
3. Layer 3 top-1 confidence is below 0.6 (high confidence = no need for embeddings)

**What gets embedded**: `name_decomposed + " " + signature + " " + doc_comment`. Never the source body. A 10-word query matched against 8000 chars of code is the fundamental mistake that killed sqmd's retrieval. Match against the symbol's *identity* instead.

**Scoring blend:**
```
if embedding_activated:
    query_embedding = embed(query)   # cached per session
    for candidate in top_50:
        cand_embedding = symbol_vec[candidate.id]
        cos_sim = cosine_similarity(query_embedding, cand_embedding)
        candidate.score = candidate.score * 0.7 + cos_sim * 0.3
    re-sort top_50
```

The 70/30 blend means embeddings can reorder but never override. A symbol that ranked #1 by heuristic score can drop to #3 if embeddings strongly disagree, but can't disappear entirely.

**Query embedding cache**: The query embedding is computed once per session and cached by query hash. If the user searches "rate limit" twice, the second search skips the embed call entirely.

Returns: final top_k results (default 10).

---

## Hot Context Cache

The hot cache is an in-memory read-through layer that sits between the search API and SQLite. Its job: make repeated queries and context assembly instant by keeping warm the data that gets accessed most.

### What Gets Cached

**Symbol neighborhoods** — the big one. For hot symbols, pre-fetch their 1-hop graph context:

```rust
pub struct Neighborhood {
    pub symbol: Symbol,
    pub callers: Vec<(Symbol, f64)>,     // symbols that call this one, with path weight
    pub callees: Vec<(Symbol, f64)>,     // symbols this one calls
    pub members: Vec<Symbol>,            // contained symbols (class methods, etc.)
    pub container: Option<Symbol>,       // containing symbol (class for method)
    pub implementors: Vec<Symbol>,       // types implementing this interface
    pub parents: Vec<Symbol>,            // types this extends/implements
    pub importers: Vec<File>,            // files importing this symbol's file
    pub imports: Vec<File>,              // files this symbol's file imports
    pub tests: Vec<Symbol>,              // test symbols for this one
}
```

When Layer 2 (structural expansion) runs, it checks the cache first. A cached neighborhood means zero graph traversal for that symbol — just pointer dereferences.

**Search result sets** — LRU cache of recent queries:

```rust
pub struct ResultCache {
    cache: LruCache<QueryHash, Vec<ScoredSymbol>>,
    hit_count: usize,
}
```

Exact query match → instant return. Also supports fuzzy overlap: if the new query shares >70% of tokens with a cached query, the cached result set is used as a warm start (re-scored, not returned verbatim).

**Blast radii** — cached by (symbol_id, direction, depth):

```
blast_cache.get((1234, Forward, 3)) → Option<Vec<BlastEntry>>
```

Blast radius is the most expensive graph operation (bounded BFS). Caching it means repeated blast queries on the same symbol are free.

**Source text** — the actual source code for recently-accessed symbols:

```
source_cache.get(symbol_id) → Option<String>
```

Avoids re-reading from SQLite for context assembly. Symbols that appear in results are automatically cached.

**Assembled retrieval context** — the fully composed context for hot neighborhoods, not just the raw graph edges. This is the layer agents hammer repeatedly: "give me everything around X."

```rust
pub struct AssembledContext {
    pub symbol: Symbol,
    pub source: String,
    pub signature_context: String,  // "class RateLimiter implements IMiddleware { ... }"
    pub callers_summary: String,    // "Called by: server.ts::setupMiddleware, chain.ts::execute"
    pub callees_summary: String,    // "Calls: checkLimit(), TokenBucket.consume(), getConfig()"
    pub test_summary: Option<String>, // "Tests: rateLimit.test.ts::testRateLimit, testRateLimitExceeded"
    pub file_context: String,       // imports/exports of the containing file
    pub assembled_at: Instant,      // for staleness checks
}
```

The difference from a raw `Neighborhood`: this is the *rendered* context ready to be injected into an LLM prompt or returned to a caller. The raw graph tells you `RateLimiter` calls `checkLimit()`. The assembled context tells you *what that looks like as a paragraph of text*. Caching this means repeated "explain RateLimiter" or "context for RateLimiter" calls skip graph traversal *and* string assembly — just return the cached blob.

Populated on-demand: when a symbol is accessed ≥3 times in a session, or when explicitly requested. Evicted on reindex for that symbol's file.

### Cache Architecture

```rust
pub struct HotCache {
    neighborhoods: DashMap<i64, Neighborhood>,          // symbol_id → 1-hop graph
    assembled: DashMap<i64, AssembledContext>,           // symbol_id → rendered context for agents
    results: Mutex<LruCache<u64, Vec<ScoredSymbol>>>,   // query_hash → results
    blast: DashMap<BlastKey, Vec<BlastEntry>>,           // (symbol_id, dir, depth) → blast
    source: DashMap<i64, String>,                        // symbol_id → source text

    stats: CacheStats,                                   // hit rates, eviction counts per cache type
    config: CacheConfig,
}

pub struct CacheConfig {
    pub max_neighborhoods: usize,   // default: 10_000
    pub max_assembled: usize,       // default: 2_000
    pub max_results: usize,         // default: 500
    pub max_blast: usize,           // default: 1_000
    pub max_source: usize,          // default: 5_000
    pub prewarm_top_n: usize,       // default: 200 (pre-warm top-N important symbols on index)
    pub assembly_threshold: usize,  // default: 3 (accesses before assembling cached context)
}
```

Thread-safe (`DashMap` for concurrent reads, `Mutex` for LRU). Sized for ~50MB max in typical usage.

### Cache Lifecycle

**Population** (on-demand + pre-warm):
1. **Pre-warm on index**: After indexing completes, compute neighborhoods for the top 200 symbols by importance score. These are the symbols most likely to appear in results.
2. **On-demand fill**: When Layer 2 expands a symbol not in cache, fetch its neighborhood from SQLite, cache it, then use it.
3. **Result caching**: After each search completes, cache the result set keyed by query hash.
4. **Blast caching**: After computing blast radius, cache the result.

**Eviction**:
- LRU for result sets and blast caches
- Size-bounded for neighborhoods and source text (evict least-recently-used when at capacity)
- Explicit invalidation on reindex: when files change, evict all cache entries for symbols in those files

**Invalidation on reindex**:
```
for changed_file in reindexed_files:
    symbol_ids = SELECT id FROM symbols WHERE file_id = changed_file.id
    for id in symbol_ids:
        cache.neighborhoods.remove(id)
        cache.source.remove(id)
    cache.blast.retain(|key, _| key.symbol_id not in symbol_ids)
    cache.results.clear()  // results may be stale, full clear
```

### Cache Performance Targets

| Scenario | Cold | Warm (cached) |
|----------|------|---------------|
| Exact repeat query | 30-50ms | < 1ms |
| Related query (70%+ token overlap) | 30-50ms | < 5ms |
| Structural expansion for hot symbol | 10-20ms | < 1ms |
| Blast radius (depth 3) | 30-50ms | < 1ms |
| Context assembly (10 symbols) | 20-40ms | < 5ms |

The cache makes GraphIQ feel instant for interactive use. First query pays the full pipeline cost. Every subsequent query in the same session is nearly free if it touches overlapping code.

---

## Blast Radius

A first-class operation, not a bolt-on.

### Boundary Contract: graph.rs vs blast.rs

These two modules share machinery (graph traversal, edge queries) but serve fundamentally different purposes. The boundary must stay sharp or they'll collapse into one incoherent traversal system.

| | `graph.rs` | `blast.rs` |
|---|---|---|
| **Question** | "What related things help retrieve the answer?" | "What things are affected if this changes?" |
| **Direction** | Outward from FTS hits — any edge type, breadth-first | Forward (dependents) and backward (dependencies) — selective edge types |
| **Depth** | 1-2 hops max, wide | Configurable (1-5 hops), deep |
| **Edge filter** | All edges welcome — `Contains`, `Implements`, `Tests` all help retrieval | Only dependency edges: `Calls`, `Imports`, `References`, `Contains` — skip `Tests`, `Implements` for forward blast |
| **Scoring** | Decay + path weight, candidates merged into ranked list | No scoring — exhaustive enumeration, grouped by distance |
| **Output** | Scored candidate symbols for the funnel | Structured impact report (tree + counts) |
| **Caching** | Via `Neighborhood` in hot cache (1-hop) | Via `blast_cache` in hot cache (full BFS result) |

**Shared infrastructure** (lives in a common `traverse` function both call):
```rust
// Both graph.rs and blast.rs use this primitive. Neither owns it.
fn bounded_bfs(
    db: &Connection,
    start_ids: &[i64],
    direction: TraverseDirection,  // Outgoing or Incoming
    edge_filter: &[EdgeKind],
    max_depth: usize,
) -> Vec<(i64, usize, Vec<EdgeKind>)>  // (symbol_id, distance, path)
```

`graph.rs` calls `bounded_bfs` with `direction=Outgoing`, all edge types, depth 2, then scores the results.
`blast.rs` calls `bounded_bfs` twice (forward + backward) with selective edge types and configurable depth, then formats the report.

**The rule**: if you're producing candidates for ranking, it's `graph.rs`. If you're producing an impact report, it's `blast.rs`. If you're not sure, you're about to blur the boundary — stop and pick one.

### API

```rust
pub struct BlastRadius {
    pub origin: Symbol,
    pub forward: Vec<BlastEntry>,   // what this symbol affects
    pub backward: Vec<BlastEntry>,  // what this symbol depends on
    pub forward_count: usize,
    pub backward_count: usize,
    pub max_depth: usize,
}

pub struct BlastEntry {
    pub symbol: Symbol,
    pub distance: usize,
    pub path: Vec<(i64, EdgeKind)>,  // (symbol_id, edge_kind) path from origin
}

pub fn compute_blast_radius(
    db: &Connection,
    symbol_id: i64,
    max_depth: usize,
    direction: BlastDirection,  // Forward, Backward, or Both
    edge_filter: Option<Vec<EdgeKind>>,  // optionally filter by edge type
) -> BlastRadius
```

### Implementation

BFS on the `edges` table using recursive CTE:

```sql
-- Forward blast radius (what does this symbol affect?)
WITH RECURSIVE blast AS (
    SELECT target_id AS symbol_id, 1 AS distance, 
           JSON_ARRAY(kind) AS path
    FROM edges WHERE source_id = ?
    
    UNION ALL
    
    SELECT e.target_id, b.distance + 1,
           JSON_INSERT(b.path, '$[#]', e.kind)
    FROM edges e 
    JOIN blast b ON e.source_id = b.symbol_id
    WHERE b.distance < ?
      AND e.kind IN ('imports', 'calls', 'references')
)
SELECT b.symbol_id, b.distance, b.path,
       s.name, s.kind, s.file_id, f.path
FROM blast b
JOIN symbols s ON s.id = b.symbol_id
JOIN files f ON f.id = s.file_id
ORDER BY b.distance, s.importance DESC;

-- Backward blast radius (what does this symbol depend on?)
-- Same query but with source_id/target_id swapped
```

### Blast Radius Report

For a given change (symbol or file), generate a human-readable report:

```
Blast Radius: RateLimiter.handle()
├── Forward (affects):
│   ├── [1] api/server.ts::setupMiddleware() (calls)
│   ├── [1] middleware/chain.ts::execute() (calls)
│   ├── [2] routes/handler.ts::processRequest() (calls via server.ts)
│   └── [2] tests/middleware.test.ts::testRateLimit() (tests)
├── Backward (depends on):
│   ├── [1] RateLimiter.checkLimit() (calls)
│   ├── [1] TokenBucket.consume() (calls)
│   ├── [1] config/rateLimit.ts::getConfig() (calls)
│   └── [2] RedisClient.get() (calls via TokenBucket)
└── Summary: 4 forward, 4 backward, max depth 2
```

---

## Search API

```rust
pub struct SearchQuery {
    pub query: String,
    pub top_k: usize,                        // default: 10
    pub max_expansion_depth: usize,          // default: 2
    pub include_callers: bool,               // default: true
    pub include_callees: bool,               // default: true
    pub include_tests: bool,                 // default: true
    pub include_dependencies: bool,          // default: true
    pub blast_radius: bool,                  // default: false
    pub use_embeddings: bool,                // default: false
    pub file_filter: Option<String>,         // e.g., "src/middleware/"
    pub kind_filter: Option<Vec<SymbolKind>>, // e.g., [Function, Method]
    pub language_filter: Option<Vec<String>>, // e.g., ["typescript", "rust"]
}

pub struct SearchResult {
    pub symbol: Symbol,
    pub score: f64,
    pub match_source: MatchSource,           // FTS, Structural, Embedding
    pub expansion_path: Option<Vec<Edge>>,   // how we got here from FTS hit
    pub blast_radius: Option<BlastRadius>,   // if requested
}

pub enum MatchSource {
    FTS { bm25_score: f64 },
    Structural { origin_symbol_id: i64, distance: usize, edge_kinds: Vec<EdgeKind> },
    Embedding { cosine_similarity: f64 },
}
```

---

## CLI

```bash
# Index a project
graphiq index /path/to/project

# Search
graphiq search "rate limit middleware"
graphiq search "authenticateUser" --kind function
graphiq search "error handler" --lang typescript --top 20

# Blast radius
graphiq blast src/middleware/rateLimit.ts::RateLimiter --depth 3
graphiq blast RateLimiter.handle --direction forward
graphiq blast RateLimiter --edge-type imports,calls

# Status
graphiq status   # files, symbols, edges, index size, last indexed

# Reindex
graphiq reindex  # incremental — only changed files

# (Optional) Enable embeddings
graphiq embed --model gte-modernbert-base
```

---

## Benchmarking Strategy

### Metrics

- **MRR (Mean Reciprocal Rank)**: primary metric. For each query, `1/rank` of the first correct result. Average across all queries.
- **Hit@K**: percentage of queries where a correct result appears in top K (K=1,3,5,10).
- **Latency**: p50, p95, p99 for search queries.

### Benchmark Classes

Averaging all queries together produces meaningless numbers. Define classes upfront so you're comparing apples to apples.

**By query type:**

| Class | Example | Expected Difficulty | Target MRR |
|-------|---------|---------------------|------------|
| `symbol-exact` | `"RateLimiter"`, `"authenticateUser"` | Easy — FTS should nail this | > 0.90 |
| `symbol-partial` | `"rate lim"`, `"auth user"` | Medium — prefix + decomposition | > 0.70 |
| `nl-descriptive` | `"rate limiting middleware"`, `"how does auth work"` | Hard — needs structural expansion | > 0.50 |
| `nl-abstract` | `"how are errors propagated"`, `"request lifecycle"` | Hardest — needs multi-hop graph | > 0.30 |
| `file-path` | `"middleware/rateLimit"`, `"src/auth"` | Easy — path tokenization | > 0.85 |
| `error-debug` | `"RateLimitError"`, `"too many requests"` | Medium — definition + callers | > 0.60 |
| `cross-cutting` | `"all middleware implementations"`, `"subclasses of BaseService"` | Hard — type hierarchy traversal | > 0.45 |

**By cache state:**

| Class | Description | Expected Latency |
|-------|-------------|------------------|
| `cold` | Fresh session, empty cache | Full pipeline cost |
| `warm-neighborhood` | Hot symbols pre-cached, first query | FTS + cached expansion |
| `warm-repeat` | Exact or fuzzy repeat of previous query | < 1ms (result cache) |
| `warm-context` | Assembled context for recently-accessed symbol | < 1ms (assembled cache) |

**By repository scale:**

| Class | Files | Symbols | Edges | Expected p95 |
|-------|-------|---------|-------|-------------|
| `small` | < 100 | < 2K | < 5K | < 30ms cold |
| `medium` | 100-1000 | 2K-20K | 5K-50K | < 50ms cold |
| `large` | 1000-5000 | 20K-100K | 50K-500K | < 100ms cold |
| `monorepo` | > 5000 | > 100K | > 500K | < 200ms cold |

**The rule**: every latency number must be reported with (query class, cache state, repo scale). "p95 < 100ms" means nothing without those three qualifiers.

### Target Performance (per class)

**`medium` repo, `cold` cache:**

| Query Class | MRR Target | Hit@1 | Hit@5 | Hit@10 | p95 Latency |
|-------------|-----------|-------|-------|--------|-------------|
| `symbol-exact` | > 0.90 | > 85% | > 95% | > 98% | < 15ms |
| `symbol-partial` | > 0.70 | > 55% | > 85% | > 92% | < 20ms |
| `nl-descriptive` | > 0.50 | > 35% | > 65% | > 80% | < 50ms |
| `nl-abstract` | > 0.30 | > 15% | > 45% | > 65% | < 80ms |
| `file-path` | > 0.85 | > 75% | > 90% | > 95% | < 15ms |
| `error-debug` | > 0.60 | > 45% | > 75% | > 88% | < 25ms |
| `cross-cutting` | > 0.45 | > 25% | > 60% | > 75% | < 50ms |
| **Overall** | **> 0.60** | **> 50%** | **> 80%** | **> 90%** | **< 100ms** |

**`warm-repeat` cache (any repo scale):**

All query classes: p95 < 1ms. This is the cache doing its job.

The overall targets are aggressive but achievable because:
1. `symbol-exact` and `file-path` carry most queries in practice and should be near-perfect
2. `nl-descriptive` is where structural expansion earns its keep
3. `nl-abstract` is the hardest class — acceptable to be weaker as long as it doesn't drag down the overall
4. Cache makes repeated queries in a session effectively free

---

## Dependencies

```toml
[dependencies]
rusqlite = { version = "0.31", features = ["bundled", "collation"] }
tree-sitter = "0.24"
tree-sitter-typescript = "0.23"
tree-sitter-rust = "0.23"
tree-sitter-python = "0.23"
tree-sitter-go = "0.23"
tree-sitter-java = "0.23"
tree-sitter-c = "0.23"
tree-sitter-cpp = "0.23"
tree-sitter-ruby = "0.23"
tree-sitter-yaml = "0.7"
tree-sitter-json = "0.24"
tree-sitter-toml-ng = "0.6"
tree-sitter-html = "0.23"
tree-sitter-css = "0.23"
rayon = "1.10"
dashmap = "6.1"
lru = "0.12"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
clap = { version = "4", features = ["derive"] }

# Optional: embedding support
# llama-cpp-2 = { version = "0.1", optional = true }
# sqlite-vec = { version = "0.1", optional = true }

[features]
default = []
embed = []  # ["llama-cpp-2", "sqlite-vec"]
```

---

## Implementation Order

### Phase 1: Skeleton (Day 1-2)
- [ ] Cargo workspace setup
- [ ] `symbol.rs`, `edge.rs` — core types
- [ ] `db.rs` — SQLite schema v1, migrations, basic CRUD
- [ ] `files.rs` — project walking, language detection (port from sqmd)

### Phase 2: Parsing (Day 3-5)
- [ ] `chunker.rs` — LanguageChunker trait (port from sqmd, clean)
- [ ] `languages/*.rs` — port TreeSitter parsers (rename Chunk → Symbol)
- [ ] `calls.rs` — call site extraction (port from sqmd)
- [ ] `tokenize.rs` — identifier decomposition
- [ ] `index.rs` — indexing pipeline orchestrator

### Phase 3: Retrieval (Day 6-9)
- [ ] `fts.rs` — BM25 query builder, FTS search
- [ ] `graph.rs` — graph traversal, structural expansion
- [ ] `rerank.rs` — cheap rerank: path weights, heuristics, diversity dampen
- [ ] `cache.rs` — hot context cache (neighborhoods, result LRU, blast, source)
- [ ] `search.rs` — unified funnel API
- [ ] `blast.rs` — blast radius computation

### Phase 4: CLI + Bench (Day 10-12)
- [ ] `graphiq-cli` — index, search, blast, status, cache-status commands
- [ ] `graphiq-bench` — MRR/Hit@K benchmarking
- [ ] Benchmark against sqmd baseline queries
- [ ] Cache hit-rate benchmarking

### Phase 5: Polish (Day 11-14)
- [ ] Embedding support (optional feature)
- [ ] MCP server for LLM integration
- [ ] Documentation
- [ ] Performance optimization (query plans, index tuning)

---

## Key Design Principles

1. **FTS is the backbone.** Never skip it. It's fast, deterministic, and identifiers carry meaning.
2. **Structural indexes bridge the semantic gap.** Call graphs, import graphs, and type hierarchies provide the "understanding" that people try to get from embeddings — at zero embedding cost.
3. **Embeddings are a reranker, never a retriever.** They touch 50 candidates max, never the full corpus. And only when heuristic confidence is low.
4. **Path weights are not all equal.** A `Calls` edge is 2.5× more informative than a `References` edge. Weight accordingly.
5. **Symbols, not chunks.** Every result is a named, typed code entity with a defined scope. No "Section" noise at 0.2 importance polluting results.
6. **Blast radius is a graph operation.** BFS on edges. No magic, no heuristics. Fast and exact.
7. **graph.rs and blast.rs share machinery but never share purpose.** graph.rs retrieves. blast.rs impacts. If the boundary feels fuzzy, stop and pick one.
8. **Incremental indexing.** Content-hash dedup. Only re-parse changed files.
9. **SQLite for everything.** Proven, fast, single-file, no server. FTS5 + recursive CTEs cover everything we need.
10. **Cache hot paths aggressively — including assembled context, not just raw lookups.** Symbol neighborhoods, assembled retrieval context, recent results, blast radii. First query pays full cost; every subsequent query touching the same code is nearly free.
11. **Cheap rerank before expensive rerank.** Path weights and heuristics are deterministic and free. Run them first, then only touch embeddings if heuristic confidence is low.
12. **Every heuristic is toggleable and logged.** When retrieval feels off, you need to know which signal helped and which hurt. Debug mode prints score breakdowns per result.
13. **Caches in SQLite are disposable.** `blast_cache` is fully recomputable from the `edges` table. Nothing depends on it being fresh. If in doubt, truncate and recompute. It is never a source of truth.
14. **Benchmark by class, not average.** Every latency number carries (query type, cache state, repo scale). "p95 < 100ms" without those three qualifiers means nothing.

---

## Roadmap

### Phase 1: Behavioral Role Tags

Add derived tags like `validator`, `cache`, `retry`, `auth-gate`, `serializer`. Infer them from paths, symbol kinds, callees, imports, and nearby tests. Store as lightweight indexed metadata on symbols. Add tag overlap as a rerank feature for abstract queries.

### Phase 2: Concept-Shaped Neighborhoods

Define a small set of structural motifs — guard, transform, orchestration, persistence. Detect motifs from local edge patterns and surrounding symbol types. Cache 1-hop or 2-hop neighborhood fingerprints for hot symbols. Score candidates by motif match, not just direct lexical hit.

### Phase 3: Query-to-Evidence Decomposition

Map abstract phrases to expected repo evidence — errors, retries, parsers, caches, tests. Expand one abstract query into a few concrete subqueries. Run those through normal FTS plus graph expansion. Merge results and reward symbols hit by multiple evidence tracks.

### Phase 4: Multi-Evidence Agreement

Keep separate scores for lexical, structural, test, path, and role evidence. Boost candidates that score across multiple channels. Penalize one-channel flukes and same-file pileups. Expose score breakdowns in debug so tuning stays honest.

### Phase 5: nl-abstract as Evidence Recovery

Only enable this path when the query looks abstract. Leave the default fast path untouched for normal recall. Make the abstract path additive, not a replacement. Benchmark it separately so it cannot quietly degrade baseline.
