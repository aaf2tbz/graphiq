#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleTag {
    Validator,
    Cache,
    Retry,
    AuthGate,
    Serializer,
    Parser,
    Builder,
    Handler,
    Middleware,
    Router,
    Guard,
    Transform,
    Emitter,
    Listener,
    Loader,
    Init,
    Cleanup,
    Config,
    Logger,
}

impl RoleTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoleTag::Validator => "validator",
            RoleTag::Cache => "cache",
            RoleTag::Retry => "retry",
            RoleTag::AuthGate => "auth gate",
            RoleTag::Serializer => "serializer",
            RoleTag::Parser => "parser",
            RoleTag::Builder => "builder",
            RoleTag::Handler => "handler",
            RoleTag::Middleware => "middleware",
            RoleTag::Router => "router",
            RoleTag::Guard => "guard",
            RoleTag::Transform => "transform",
            RoleTag::Emitter => "emitter",
            RoleTag::Listener => "listener",
            RoleTag::Loader => "loader",
            RoleTag::Init => "initializer",
            RoleTag::Cleanup => "cleanup",
            RoleTag::Config => "config",
            RoleTag::Logger => "logger",
        }
    }

    pub fn fts_terms(&self) -> &'static str {
        match self {
            RoleTag::Validator => "validate validation check verify",
            RoleTag::Cache => "cache caching cached memo memoize",
            RoleTag::Retry => "retry retries backoff reattempt",
            RoleTag::AuthGate => "auth authenticate authorization token permission",
            RoleTag::Serializer => "serialize deserialize marshal unmarshal encode decode",
            RoleTag::Parser => "parse parsing parser parses",
            RoleTag::Builder => "build builder constructing construct",
            RoleTag::Handler => "handle handler handles process processing",
            RoleTag::Middleware => "middleware chain interceptor",
            RoleTag::Router => "route routing router dispatch",
            RoleTag::Guard => "guard protect protection shield barrier",
            RoleTag::Transform => "transform mapping convert conversion",
            RoleTag::Emitter => "emit dispatch publish event",
            RoleTag::Listener => "listen subscribe subscriber",
            RoleTag::Loader => "load loader loading fetch",
            RoleTag::Init => "init initialize setup bootstrap",
            RoleTag::Cleanup => "cleanup teardown shutdown dispose",
            RoleTag::Config => "config configuration setting options",
            RoleTag::Logger => "log logging logger trace debug info warn error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoleEvidence {
    pub name: String,
    pub name_decomposed: String,
    pub file_path: Option<String>,
    pub callee_names: Vec<String>,
    pub caller_names: Vec<String>,
    pub outgoing_edge_kinds: Vec<String>,
    pub container_name: Option<String>,
}

pub fn infer_roles(evidence: &RoleEvidence) -> Vec<RoleTag> {
    let mut roles = Vec::new();
    let name_lower = evidence.name.to_lowercase();
    let decomp_lower = evidence.name_decomposed.to_lowercase();
    let path_lower = evidence.file_path.as_deref().unwrap_or("").to_lowercase();

    let callee_lower: Vec<String> = evidence
        .callee_names
        .iter()
        .map(|n| n.to_lowercase())
        .collect();

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["validate", "valid", "check", "verify"],
    ) || callee_has(&callee_lower, &["validate", "verify", "check"])
        || path_lower.contains("valid")
    {
        roles.push(RoleTag::Validator);
    }

    if matches_name(&name_lower, &decomp_lower, &["cache", "memo", "lru"])
        || callee_has(
            &callee_lower,
            &["cache", "get_cache", "set_cache", "memoize"],
        )
        || path_lower.contains("cache")
    {
        roles.push(RoleTag::Cache);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["retry", "backoff", "reattempt"],
    ) || callee_has(&callee_lower, &["retry", "backoff", "sleep"])
    {
        roles.push(RoleTag::Retry);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["auth", "login", "token", "permission", "credential"],
    ) || callee_has(
        &callee_lower,
        &["authenticate", "verify_token", "authorize"],
    ) || path_lower.contains("auth")
    {
        roles.push(RoleTag::AuthGate);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["serialize", "deserialize", "marshal", "encode", "decode"],
    ) || callee_has(
        &callee_lower,
        &["serialize", "to_json", "from_json", "encode", "decode"],
    ) {
        roles.push(RoleTag::Serializer);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["parse", "read", "scan", "lex"],
    ) || callee_has(&callee_lower, &["parse", "scan", "tokenize"])
        || path_lower.contains("parse")
    {
        roles.push(RoleTag::Parser);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["build", "construct", "create"],
    ) || callee_has(&callee_lower, &["new", "create", "build", "default"])
    {
        roles.push(RoleTag::Builder);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["handle", "process", "dispatch", "execute", "run"],
    ) {
        roles.push(RoleTag::Handler);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["middleware", "intercept", "chain"],
    ) || path_lower.contains("middleware")
    {
        roles.push(RoleTag::Middleware);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["route", "router", "dispatch", "endpoint"],
    ) || path_lower.contains("route")
    {
        roles.push(RoleTag::Router);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["guard", "protect", "shield", "barrier", "prevent"],
    ) || callee_has(&callee_lower, &["guard", "protect", "block", "reject"])
    {
        roles.push(RoleTag::Guard);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["transform", "convert", "map", "translate", "adapt"],
    ) || callee_has(&callee_lower, &["transform", "convert", "map"])
    {
        roles.push(RoleTag::Transform);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["emit", "dispatch", "publish", "notify"],
    ) || callee_has(&callee_lower, &["emit", "dispatch", "publish"])
    {
        roles.push(RoleTag::Emitter);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["listen", "subscribe", "watch", "observe"],
    ) {
        roles.push(RoleTag::Listener);
    }

    if matches_name(&name_lower, &decomp_lower, &["load", "fetch", "import"])
        || callee_has(&callee_lower, &["load", "fetch", "read_file"])
        || path_lower.contains("loader")
    {
        roles.push(RoleTag::Loader);
    }

    if matches_name(&name_lower, &decomp_lower, &["init", "setup", "bootstrap"]) {
        roles.push(RoleTag::Init);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["cleanup", "teardown", "shutdown", "dispose", "drop"],
    ) {
        roles.push(RoleTag::Cleanup);
    }

    if matches_name(&name_lower, &decomp_lower, &["config", "setting", "option"])
        || path_lower.contains("config")
    {
        roles.push(RoleTag::Config);
    }

    if matches_name(
        &name_lower,
        &decomp_lower,
        &["log", "trace", "debug", "info", "warn", "error"],
    ) || callee_has(
        &callee_lower,
        &["log", "trace", "debug", "info", "warn", "error"],
    ) || path_lower.contains("log")
    {
        roles.push(RoleTag::Logger);
    }

    roles.truncate(5);
    roles
}

pub fn roles_to_hints(roles: &[RoleTag]) -> String {
    let role_terms: Vec<&str> = roles.iter().map(|r| r.fts_terms()).collect();
    let role_names: Vec<&str> = roles.iter().map(|r| r.as_str()).collect();
    let mut parts = Vec::new();
    parts.push(role_terms.join(" "));
    parts.push(format!("role: {}", role_names.join(", ")));
    parts.join(". ")
}

fn matches_name(name_lower: &str, decomp_lower: &str, patterns: &[&str]) -> bool {
    patterns
        .iter()
        .any(|p| name_lower.contains(p) || decomp_lower.contains(p))
}

fn callee_has(callee_lower: &[String], patterns: &[&str]) -> bool {
    callee_lower
        .iter()
        .any(|c| patterns.iter().any(|p| c.contains(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_evidence(name: &str) -> RoleEvidence {
        RoleEvidence {
            name: name.into(),
            name_decomposed: crate::tokenize::decompose_identifier(name),
            file_path: None,
            callee_names: Vec::new(),
            caller_names: Vec::new(),
            outgoing_edge_kinds: Vec::new(),
            container_name: None,
        }
    }

    #[test]
    fn test_infer_validator() {
        let ev = RoleEvidence {
            callee_names: vec!["validateInput".into()],
            ..make_evidence("processForm")
        };
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::Validator));
    }

    #[test]
    fn test_infer_cache_from_path() {
        let ev = RoleEvidence {
            file_path: Some("src/cache/lru.rs".into()),
            ..make_evidence("LRUCache")
        };
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::Cache));
    }

    #[test]
    fn test_infer_handler() {
        let ev = make_evidence("handleClick");
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::Handler));
    }

    #[test]
    fn test_role_cap() {
        let ev = RoleEvidence {
            callee_names: vec![
                "validate".into(),
                "cache".into(),
                "retry".into(),
                "serialize".into(),
                "parse".into(),
                "build".into(),
            ],
            ..make_evidence("doEverything")
        };
        let roles = infer_roles(&ev);
        assert!(roles.len() <= 5);
    }

    #[test]
    fn test_roles_to_hints() {
        let hints = roles_to_hints(&[RoleTag::Validator, RoleTag::Cache]);
        assert!(hints.contains("validate"));
        assert!(hints.contains("cache"));
        assert!(hints.contains("role:"));
    }
}
