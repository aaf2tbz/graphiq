//! Structural roles — infer what a symbol does from its name and context.
//!
//! Assigns role tags (Validator, Cache, Handler, EntryPoint, etc.) to symbols
//! based on naming patterns, call patterns, and file paths. These tags are
//! written into the FTS search hints column so BM25 can match role vocabulary.
//!
//! A function named `ensureFreshness` that checks cache validity gets hints
//! like "cache validate check verify" — so the query "validate cache entry"
//! finds it even though the name doesn't contain those words.
//!
//! Key function: [`infer_roles`] — returns role tags for a symbol.

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
    ErrorProducer,
    ErrorPropagator,
    ErrorHandler,
    ErrorType,
    EntryPoint,
    Scheduler,
    Factory,
    Transformer,
    IO,
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
            RoleTag::ErrorProducer => "error producer",
            RoleTag::ErrorPropagator => "error propagator",
            RoleTag::ErrorHandler => "error handler",
            RoleTag::ErrorType => "error type",
            RoleTag::EntryPoint => "entry point",
            RoleTag::Scheduler => "scheduler",
            RoleTag::Factory => "factory",
            RoleTag::Transformer => "transformer",
            RoleTag::IO => "io",
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
            RoleTag::ErrorProducer => "error err fail failure throw raise panic abort",
            RoleTag::ErrorPropagator => "propagate bubble rethrow map_err chain error",
            RoleTag::ErrorHandler => "catch recover handle rescue error fallback graceful",
            RoleTag::ErrorType => "error exception fault errortype customerror apperror",
            RoleTag::EntryPoint => "entry main start init serve run bootstrap launch",
            RoleTag::Scheduler => "schedule dispatch queue enqueue defer spawn timer interval",
            RoleTag::Factory => "factory create construct new build instantiate make spawn",
            RoleTag::Transformer => "transform convert adapt map translate compose pipe",
            RoleTag::IO => "read write send recv connect accept open close socket stream file",
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
    pub signature: Option<String>,
    pub source_text: Option<String>,
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

    {
        let sig_lower = evidence.signature.as_deref().unwrap_or("").to_lowercase();
        let src_lower = evidence.source_text.as_deref().unwrap_or("").to_lowercase();

        let has_result_return = sig_lower.contains("result<")
            || sig_lower.contains("-> result")
            || sig_lower.contains("-> Result")
            || sig_lower.contains(": Result")
            || sig_lower.contains("throws")
            || sig_lower.contains(": Error");
        let has_err_construction = src_lower.contains("result::err")
            || src_lower.contains("result::from")
            || src_lower.contains("error::new")
            || src_lower.contains("throw ")
            || src_lower.contains("throw new")
            || src_lower.contains("raise ")
            || src_lower.contains("panic!(")
            || src_lower.contains("abort(")
            || src_lower.contains("anyhow!")
            || src_lower.contains("bail!")
            || src_lower.contains("ensure!(");
        let callee_err = callee_has(
            &callee_lower,
            &["error", "fail", "panic", "abort", "raise", "throw"],
        );
        if (has_err_construction || callee_err) && has_result_return {
            roles.push(RoleTag::ErrorProducer);
        }

        let has_propagation = src_lower.contains("?;")
            || src_lower.contains("? ")
            || src_lower.contains("?)")
            || src_lower.contains("?\n")
            || src_lower.contains("map_err")
            || src_lower.contains("try!")
            || src_lower.contains("catch")
            || src_lower.contains("except:");
        if has_propagation && has_result_return {
            roles.push(RoleTag::ErrorPropagator);
        }

        let has_err_param = sig_lower.contains("error")
            || sig_lower.contains(": err")
            || sig_lower.contains(": e)")
            || sig_lower.contains("exception");
        let has_err_match = src_lower.contains("match") && src_lower.contains("err")
            || src_lower.contains("ok(")
            || src_lower.contains("catch (")
            || src_lower.contains("except ");
        if (has_err_param || has_err_match) && !has_propagation {
            roles.push(RoleTag::ErrorHandler);
        }
    }

    {
        let kind_lower = evidence
            .outgoing_edge_kinds
            .iter()
            .map(|k| k.to_lowercase())
            .collect::<Vec<_>>();
        let has_calls_out = kind_lower.iter().any(|k| k == "calls");
        let has_calls_in = evidence.caller_names.len() > 0;
        let call_out_count = evidence.callee_names.len();
        if !has_calls_in && has_calls_out && call_out_count >= 3 {
            roles.push(RoleTag::EntryPoint);
        }

        if has_calls_out {
            let callee_files: std::collections::HashSet<&str> = evidence
                .callee_names
                .iter()
                .take(10)
                .filter_map(|n| n.split('.').next())
                .collect();
            let unique_file_roots = callee_files.len();
            if unique_file_roots >= 3 && call_out_count >= 3 {
                roles.push(RoleTag::Scheduler);
            }
        }
    }

    {
        let sig_lower = evidence.signature.as_deref().unwrap_or("").to_lowercase();
        let returns_new = sig_lower.contains("-> self")
            || sig_lower.contains("-> struct")
            || sig_lower.contains(": Self")
            || sig_lower.contains("-> new")
            || matches_name(
                &name_lower,
                &decomp_lower,
                &["new", "create", "from", "spawn"],
            );
        if returns_new || callee_has(&callee_lower, &["new", "create", "default", "alloc"]) {
            roles.push(RoleTag::Factory);
        }

        let has_transform = sig_lower.contains("-> &")
            || sig_lower.contains("-> option")
            || sig_lower.contains("-> result")
            || sig_lower.contains("-> vec")
            || sig_lower.contains("-> string")
            || sig_lower.contains("-> map");
        if has_transform
            && !matches_name(
                &name_lower,
                &decomp_lower,
                &["get", "set", "is", "has", "can"],
            )
        {
            roles.push(RoleTag::Transformer);
        }
    }

    {
        let callee_lower_io: Vec<String> = evidence
            .callee_names
            .iter()
            .map(|n| n.to_lowercase())
            .collect();
        if callee_has(
            &callee_lower_io,
            &[
                "read", "write", "send", "recv", "connect", "accept", "open", "close", "socket",
                "stream", "flush", "tcp", "udp", "http", "fetch", "request", "query", "execute",
                "stdin", "stdout", "stderr",
            ],
        ) || matches_name(
            &name_lower,
            &decomp_lower,
            &[
                "read", "write", "send", "recv", "connect", "listen", "fetch", "request",
            ],
        ) {
            roles.push(RoleTag::IO);
        }
    }

    roles.truncate(8);
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
            signature: None,
            source_text: None,
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
        assert!(roles.len() <= 8);
    }

    #[test]
    fn test_roles_to_hints() {
        let hints = roles_to_hints(&[RoleTag::Validator, RoleTag::Cache]);
        assert!(hints.contains("validate"));
        assert!(hints.contains("cache"));
        assert!(hints.contains("role:"));
    }

    #[test]
    fn test_error_producer_from_source() {
        let ev = RoleEvidence {
            signature: Some("fn foo() -> Result<(), Error>".into()),
            source_text: Some("Result::Err(Error::new(\"bad\"))".into()),
            callee_names: vec!["fail".into()],
            ..make_evidence("processData")
        };
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::ErrorProducer));
    }

    #[test]
    fn test_error_propagator_from_source() {
        let ev = RoleEvidence {
            signature: Some("fn fetch(url: &str) -> Result<Data, Error>".into()),
            source_text: Some("let resp = client.get(url)?;".into()),
            ..make_evidence("fetchData")
        };
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::ErrorPropagator));
    }

    #[test]
    fn test_error_handler_from_match() {
        let ev = RoleEvidence {
            signature: Some("fn handle(result: Result<T, E>)".into()),
            source_text: Some("match result { Ok(v) => v, Err(e) => fallback(e) }".into()),
            ..make_evidence("process")
        };
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::ErrorHandler));
    }

    #[test]
    fn test_entry_point() {
        let ev = RoleEvidence {
            caller_names: vec![],
            callee_names: vec!["init".into(), "load_config".into(), "start_server".into()],
            outgoing_edge_kinds: vec!["calls".into(), "calls".into(), "calls".into()],
            ..make_evidence("main")
        };
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::EntryPoint));
    }

    #[test]
    fn test_io_from_callees() {
        let ev = RoleEvidence {
            callee_names: vec!["tcp_connect".into(), "send".into()],
            ..make_evidence("networkCall")
        };
        let roles = infer_roles(&ev);
        assert!(roles.contains(&RoleTag::IO));
    }
}
