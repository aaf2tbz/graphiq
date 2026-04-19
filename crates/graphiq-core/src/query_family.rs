use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryFamily {
    SymbolExact,
    SymbolPartial,
    FilePath,
    ErrorDebug,
    NaturalDescriptive,
    NaturalAbstract,
    CrossCuttingSet,
    Relationship,
}

impl fmt::Display for QueryFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryFamily::SymbolExact => write!(f, "symbol-exact"),
            QueryFamily::SymbolPartial => write!(f, "symbol-partial"),
            QueryFamily::FilePath => write!(f, "file-path"),
            QueryFamily::ErrorDebug => write!(f, "error-debug"),
            QueryFamily::NaturalDescriptive => write!(f, "nl-descriptive"),
            QueryFamily::NaturalAbstract => write!(f, "nl-abstract"),
            QueryFamily::CrossCuttingSet => write!(f, "cross-cutting"),
            QueryFamily::Relationship => write!(f, "relationship"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetrievalPolicy {
    pub family: QueryFamily,
    pub bm25_lock_strength: f64,
    pub allow_spectral: bool,
    pub allow_predictive: bool,
    pub allow_fingerprints: bool,
    pub spectral_expansion_seeds: usize,
    pub spectral_heat_scale: f64,
    pub diversity_boost: f64,
    pub evidence_weight: f64,
}

impl RetrievalPolicy {
    pub fn for_family(family: QueryFamily) -> Self {
        match family {
            QueryFamily::SymbolExact => Self {
                family,
                bm25_lock_strength: 1.0,
                allow_spectral: false,
                allow_predictive: false,
                allow_fingerprints: false,
                spectral_expansion_seeds: 0,
                spectral_heat_scale: 0.0,
                diversity_boost: 0.0,
                evidence_weight: 0.0,
            },
            QueryFamily::SymbolPartial => Self {
                family,
                bm25_lock_strength: 0.7,
                allow_spectral: true,
                allow_predictive: false,
                allow_fingerprints: false,
                spectral_expansion_seeds: 15,
                spectral_heat_scale: 3.0,
                diversity_boost: 0.5,
                evidence_weight: 0.3,
            },
            QueryFamily::FilePath => Self {
                family,
                bm25_lock_strength: 0.3,
                allow_spectral: false,
                allow_predictive: false,
                allow_fingerprints: false,
                spectral_expansion_seeds: 0,
                spectral_heat_scale: 0.0,
                diversity_boost: 0.8,
                evidence_weight: 0.0,
            },
            QueryFamily::ErrorDebug => Self {
                family,
                bm25_lock_strength: 0.5,
                allow_spectral: true,
                allow_predictive: true,
                allow_fingerprints: true,
                spectral_expansion_seeds: 20,
                spectral_heat_scale: 5.0,
                diversity_boost: 0.5,
                evidence_weight: 0.5,
            },
            QueryFamily::NaturalDescriptive => Self {
                family,
                bm25_lock_strength: 0.5,
                allow_spectral: true,
                allow_predictive: true,
                allow_fingerprints: true,
                spectral_expansion_seeds: 20,
                spectral_heat_scale: 5.0,
                diversity_boost: 0.5,
                evidence_weight: 0.5,
            },
            QueryFamily::NaturalAbstract => Self {
                family,
                bm25_lock_strength: 0.3,
                allow_spectral: true,
                allow_predictive: true,
                allow_fingerprints: true,
                spectral_expansion_seeds: 30,
                spectral_heat_scale: 7.0,
                diversity_boost: 1.0,
                evidence_weight: 1.0,
            },
            QueryFamily::CrossCuttingSet => Self {
                family,
                bm25_lock_strength: 0.3,
                allow_spectral: true,
                allow_predictive: true,
                allow_fingerprints: true,
                spectral_expansion_seeds: 30,
                spectral_heat_scale: 5.0,
                diversity_boost: 1.5,
                evidence_weight: 1.0,
            },
            QueryFamily::Relationship => Self {
                family,
                bm25_lock_strength: 0.3,
                allow_spectral: true,
                allow_predictive: false,
                allow_fingerprints: true,
                spectral_expansion_seeds: 25,
                spectral_heat_scale: 5.0,
                diversity_boost: 0.8,
                evidence_weight: 1.0,
            },
        }
    }
}

const PATH_EXTENSIONS: &[&str] = &[
    ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
    ".rb", ".yaml", ".yml", ".toml", ".json", ".xml", ".html", ".css", ".scss", ".md",
];

const CROSS_CUTTING_SIGNALS: &[&str] = &[
    "all ", "every ", "each ", "any ", "all of", "list all", "show all",
];

const CROSS_CUTTING_PLURAL_NOUNS: &[&str] = &[
    "implementations", "handlers", "providers", "routes", "guards", "migrations",
    "middlewares", "controllers", "services", "models", "types", "interfaces",
    "structures", "functions", "methods", "classes", "modules", "components",
    "endpoints", "callbacks", "listeners", "adapters", "converters", "validators",
    "serializers", "deserializers", "parsers", "renderers", "views", "schemas",
    "operations", "processors", "factories", "builders", "strategies", "plugins",
];

const ABSTRACT_PREFIXES: &[&str] = &[
    "how does ", "how do ", "how is ", "how are ", "how can ",
    "what controls ", "what determines ", "what drives ", "what governs ",
    "what manages ", "what orchestrates ", "what coordinates ",
    "how does the ", "how does a ", "how does an ",
];

const ERROR_SIGNALS: &[&str] = &[
    "error", "panic", "failed", "failure", "deadlock", "timeout", "crash",
    "exception", "abort", "refused", "overflow", "underflow", "segfault",
    "nil pointer", "null pointer", "stack overflow", "out of memory",
    "connection refused", "access denied", "permission denied",
];

const RELATIONSHIP_SIGNALS: &[&str] = &[
    "what calls ", "who calls ", "what invokes ", "who invokes ",
    "callers of ", "callees of ", "what connects ", "what links ",
    "relationship between ", "how does ", " connect to ", " relate to ",
    "dependents of ", "dependencies of ",
];

pub fn classify_query_family(query: &str) -> QueryFamily {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return QueryFamily::SymbolPartial;
    }

    let lower = trimmed.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let original_tokens: Vec<&str> = trimmed.split_whitespace().collect();

    if is_file_path(&lower, &tokens) {
        return QueryFamily::FilePath;
    }

    if is_cross_cutting(&lower, &tokens) {
        return QueryFamily::CrossCuttingSet;
    }

    if is_relationship(&lower) {
        return QueryFamily::Relationship;
    }

    if is_error_debug(&lower) {
        return QueryFamily::ErrorDebug;
    }

    if is_natural_abstract(&lower) {
        return QueryFamily::NaturalAbstract;
    }

    if is_natural_descriptive(&lower, &tokens) {
        return QueryFamily::NaturalDescriptive;
    }

    if is_symbol_exact(trimmed, &tokens, &original_tokens) {
        return QueryFamily::SymbolExact;
    }

    QueryFamily::SymbolPartial
}

fn is_file_path(lower: &str, tokens: &[&str]) -> bool {
    if PATH_EXTENSIONS.iter().any(|ext| lower.contains(ext)) {
        return true;
    }
    if tokens.len() >= 2 {
        let has_separator = tokens.iter().any(|t| t.contains('/') || t.contains('\\'));
        let no_natural_words = tokens.iter().all(|t| {
            !matches!(
                *t,
                "the" | "a" | "an" | "is" | "are" | "how" | "what" | "where"
                    | "all" | "every" | "find" | "get" | "search"
            )
        });
        if has_separator && no_natural_words {
            return true;
        }
    }
    if tokens.len() == 1 {
        let t = tokens[0];
        if t.contains('/') || t.contains('\\') {
            return true;
        }
    }
    false
}

fn is_cross_cutting(lower: &str, tokens: &[&str]) -> bool {
    let first = tokens.first().copied().unwrap_or("");
    if matches!(first, "all" | "every" | "each" | "list" | "show" | "find") {
        if CROSS_CUTTING_PLURAL_NOUNS.iter().any(|noun| lower.contains(noun)) {
            return true;
        }
        if tokens.len() >= 3 {
            return true;
        }
    }
    if lower.contains("implementations of") || lower.contains("all the ") || lower.contains("list all") {
        return true;
    }
    false
}

fn is_relationship(lower: &str) -> bool {
    let specific_patterns = [
        "what calls ", "who calls ", "what invokes ", "who invokes ",
        "callers of ", "callees of ", "what connects ", "what links ",
        "relationship between ", "dependents of ", "dependencies of ",
    ];
    for p in &specific_patterns {
        if lower.contains(p) {
            return true;
        }
    }
    if lower.starts_with("how does ") && (lower.contains(" connect ") || lower.contains(" relate ")) {
        return true;
    }
    false
}

fn is_error_debug(lower: &str) -> bool {
    ERROR_SIGNALS.iter().any(|sig| lower.contains(sig))
}

fn is_natural_abstract(lower: &str) -> bool {
    ABSTRACT_PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
}

fn is_natural_descriptive(lower: &str, tokens: &[&str]) -> bool {
    if tokens.len() < 3 {
        return false;
    }
    let has_natural_words = tokens.iter().any(|t| {
        matches!(
            *t,
            "the" | "a" | "an" | "is" | "are" | "for" | "from" | "into"
                | "using" | "before" | "after" | "during" | "with" | "without"
                | "over" | "under" | "between" | "through"
        )
    });
    if !has_natural_words {
        return false;
    }
    let has_action = tokens.iter().any(|t| {
        matches!(
            *t,
            "compute" | "calculate" | "parse" | "validate" | "check"
                | "handle" | "create" | "delete" | "update" | "build"
                | "generate" | "encode" | "decode" | "scan" | "insert"
                | "prune" | "run" | "send" | "receive" | "acquire"
                | "wait" | "spawn" | "convert" | "initialize" | "recall"
                | "extract" | "transform" | "process" | "store" | "fetch"
                | "load" | "save" | "write" | "read" | "open" | "close"
                | "connect" | "disconnect" | "authenticate" | "authorize"
                | "encrypt" | "decrypt" | "hash" | "sign" | "verify"
        )
    });
    has_action && tokens.len() >= 4
}

fn is_symbol_exact(original: &str, tokens: &[&str], original_tokens: &[&str]) -> bool {
    if tokens.len() == 1 {
        let t = original_tokens[0];
        if t.contains('_') || t.contains("::") {
            return true;
        }
        let has_upper_inside = t.chars().enumerate().any(|(i, c)| i > 0 && c.is_uppercase());
        if has_upper_inside {
            return true;
        }
    }
    if tokens.len() == 1 || (tokens.len() == 2 && original.contains('.')) {
        let no_natural = tokens.iter().all(|t| t.len() > 2 && !matches!(*t, "the" | "and" | "for" | "how" | "what"));
        let has_code_shape = original_tokens.iter().any(|t| {
            t.contains('_')
                || t.contains("::")
                || t.chars().enumerate().any(|(i, c)| i > 0 && c.is_uppercase())
                || t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if no_natural && has_code_shape && tokens.len() > 1 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_exact() {
        assert_eq!(classify_query_family("RateLimiter"), QueryFamily::SymbolExact);
        assert_eq!(classify_query_family("authenticateUser"), QueryFamily::SymbolExact);
        assert_eq!(classify_query_family("compute_trust_profile"), QueryFamily::SymbolExact);
        assert_eq!(classify_query_family("GraphDb::open"), QueryFamily::SymbolExact);
        assert_eq!(classify_query_family("recallMemories"), QueryFamily::SymbolExact);
    }

    #[test]
    fn test_symbol_partial() {
        assert_eq!(classify_query_family("embedding"), QueryFamily::SymbolPartial);
        assert_eq!(classify_query_family("trust"), QueryFamily::SymbolPartial);
        assert_eq!(classify_query_family("pipeline"), QueryFamily::SymbolPartial);
        assert_eq!(classify_query_family("chunk"), QueryFamily::SymbolPartial);
        assert_eq!(classify_query_family("cache"), QueryFamily::SymbolPartial);
    }

    #[test]
    fn test_file_path() {
        assert_eq!(classify_query_family("predictor.rs"), QueryFamily::FilePath);
        assert_eq!(classify_query_family("forge_config.rs"), QueryFamily::FilePath);
        assert_eq!(classify_query_family("memory_db.rs"), QueryFamily::FilePath);
        assert_eq!(classify_query_family("claudemd.ts"), QueryFamily::FilePath);
        assert_eq!(classify_query_family("runtime/scheduler/worker.rs"), QueryFamily::FilePath);
    }

    #[test]
    fn test_error_debug() {
        assert_eq!(classify_query_family("JsonRpcError invalid request"), QueryFamily::ErrorDebug);
        assert_eq!(classify_query_family("isTimeoutError daemon connection refused"), QueryFamily::ErrorDebug);
        assert_eq!(classify_query_family("fail_job queue processing error recovery"), QueryFamily::ErrorDebug);
        assert_eq!(classify_query_family("SemaphoreTimeoutError concurrent access limit"), QueryFamily::ErrorDebug);
        assert_eq!(classify_query_family("panic in thread pool"), QueryFamily::ErrorDebug);
        assert_eq!(classify_query_family("failed to start daemon"), QueryFamily::ErrorDebug);
    }

    #[test]
    fn test_natural_descriptive() {
        assert_eq!(classify_query_family("recall memories from the store using search"), QueryFamily::NaturalDescriptive);
        assert_eq!(classify_query_family("compute trust profile for an entity"), QueryFamily::NaturalDescriptive);
        assert_eq!(classify_query_family("build the classification prompt for memory synthesis"), QueryFamily::NaturalDescriptive);
        assert_eq!(classify_query_family("initialize checkpoint flush for persistence"), QueryFamily::NaturalDescriptive);
    }

    #[test]
    fn test_natural_abstract() {
        assert_eq!(classify_query_family("how does the memory extraction pipeline work"), QueryFamily::NaturalAbstract);
        assert_eq!(classify_query_family("how are agent sessions authenticated and authorized"), QueryFamily::NaturalAbstract);
        assert_eq!(classify_query_family("how does the tool policy system select available tools"), QueryFamily::NaturalAbstract);
        assert_eq!(classify_query_family("what controls the ranking order of search results"), QueryFamily::NaturalAbstract);
        assert_eq!(classify_query_family("what determines the eviction policy"), QueryFamily::NaturalAbstract);
    }

    #[test]
    fn test_cross_cutting() {
        assert_eq!(classify_query_family("all database initialization and migration functions"), QueryFamily::CrossCuttingSet);
        assert_eq!(classify_query_family("all embedding and vector operations"), QueryFamily::CrossCuttingSet);
        assert_eq!(classify_query_family("all JSON RPC and API error types"), QueryFamily::CrossCuttingSet);
        assert_eq!(classify_query_family("all memory lifecycle operations"), QueryFamily::CrossCuttingSet);
        assert_eq!(classify_query_family("every handler for incoming requests"), QueryFamily::CrossCuttingSet);
    }

    #[test]
    fn test_relationship() {
        assert_eq!(classify_query_family("what calls authenticateUser"), QueryFamily::Relationship);
        assert_eq!(classify_query_family("callers of RateLimiter"), QueryFamily::Relationship);
        assert_eq!(classify_query_family("how does SearchEngine connect to FtsSearch"), QueryFamily::Relationship);
    }

    #[test]
    fn test_retrieval_policy_symbol_exact_locks_bm25() {
        let p = RetrievalPolicy::for_family(QueryFamily::SymbolExact);
        assert_eq!(p.bm25_lock_strength, 1.0);
        assert!(!p.allow_spectral);
    }

    #[test]
    fn test_retrieval_policy_abstract_allows_all() {
        let p = RetrievalPolicy::for_family(QueryFamily::NaturalAbstract);
        assert!(p.allow_spectral);
        assert!(p.allow_predictive);
        assert!(p.allow_fingerprints);
        assert!(p.bm25_lock_strength < 0.5);
    }

    #[test]
    fn test_retrieval_policy_cross_cutting_high_diversity() {
        let p = RetrievalPolicy::for_family(QueryFamily::CrossCuttingSet);
        assert!(p.diversity_boost > 1.0);
    }
}
