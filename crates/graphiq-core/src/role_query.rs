use crate::roles::RoleTag;

pub fn query_to_expected_roles(query: &str) -> Vec<RoleTag> {
    let lower = query.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.len() < 2 {
        return Vec::new();
    }

    let stripped = strip_query_prefix(&lower);
    let stripped_tokens: Vec<&str> = stripped.split_whitespace().collect();
    if stripped_tokens.len() < 2 {
        return Vec::new();
    }

    let mut roles = Vec::new();

    if matches_error_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::ErrorProducer);
        roles.push(RoleTag::ErrorPropagator);
        roles.push(RoleTag::ErrorHandler);
    }

    if matches_schedule_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Scheduler);
    }

    if matches_create_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Factory);
        roles.push(RoleTag::Builder);
    }

    if matches_validate_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Validator);
    }

    if matches_transform_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Transformer);
        roles.push(RoleTag::Transform);
    }

    if matches_io_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::IO);
    }

    if matches_init_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::EntryPoint);
        roles.push(RoleTag::Init);
    }

    if matches_cleanup_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Cleanup);
    }

    if matches_route_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Router);
        roles.push(RoleTag::Middleware);
    }

    if matches_auth_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::AuthGate);
        roles.push(RoleTag::Guard);
    }

    if matches_parse_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Parser);
    }

    if matches_serialize_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Serializer);
    }

    if matches_cache_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Cache);
    }

    if matches_log_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Logger);
    }

    if matches_event_intent(&lower, &stripped_tokens) {
        roles.push(RoleTag::Emitter);
        roles.push(RoleTag::Listener);
    }

    let mut seen = std::collections::HashSet::new();
    roles.retain(|r| seen.insert(*r));
    roles
}

pub fn role_boost_fts_query(query: &str, roles: &[RoleTag]) -> String {
    if roles.is_empty() {
        return query.to_string();
    }

    let role_terms: Vec<&str> = roles.iter().map(|r| r.fts_terms()).collect();
    let role_terms_flat: Vec<&str> = role_terms
        .iter()
        .flat_map(|t| t.split_whitespace())
        .take(20)
        .collect();

    format!("{} {}", query, role_terms_flat.join(" "))
}

pub fn is_role_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.len() < 2 {
        return false;
    }

    let stripped = strip_query_prefix(&lower);
    let stripped_tokens: Vec<&str> = stripped.split_whitespace().collect();
    if stripped_tokens.len() < 2 {
        return false;
    }

    matches_error_intent(&lower, &stripped_tokens)
        || matches_schedule_intent(&lower, &stripped_tokens)
        || matches_create_intent(&lower, &stripped_tokens)
        || matches_validate_intent(&lower, &stripped_tokens)
        || matches_transform_intent(&lower, &stripped_tokens)
        || matches_io_intent(&lower, &stripped_tokens)
        || matches_init_intent(&lower, &stripped_tokens)
        || matches_cleanup_intent(&lower, &stripped_tokens)
        || matches_route_intent(&lower, &stripped_tokens)
        || matches_auth_intent(&lower, &stripped_tokens)
        || matches_parse_intent(&lower, &stripped_tokens)
        || matches_serialize_intent(&lower, &stripped_tokens)
        || matches_cache_intent(&lower, &stripped_tokens)
        || matches_log_intent(&lower, &stripped_tokens)
        || matches_event_intent(&lower, &stripped_tokens)
}

fn strip_query_prefix(lower: &str) -> String {
    let prefixes = [
        "how does ",
        "how do ",
        "how are ",
        "how is ",
        "how can ",
        "what is ",
        "what are ",
        "what does ",
        "what connects ",
        "where is ",
        "where are ",
        "where does ",
        "why does ",
        "why is ",
        "why are ",
        "when does ",
        "when is ",
        "the ",
        "a ",
        "an ",
    ];
    let mut s = lower.trim().to_string();
    for prefix in &prefixes {
        if s.starts_with(prefix) {
            s = s[prefix.len()..].to_string();
        }
    }
    let suffixes = [
        " work",
        " happen",
        " occur",
        " get",
        " function",
        " implemented",
        " processed",
        " computed",
    ];
    for suffix in &suffixes {
        if s.ends_with(suffix) {
            s = s[..s.len() - suffix.len()].to_string();
        }
    }
    s.trim().to_string()
}

fn token_matches(tokens: &[&str], candidates: &[&str]) -> bool {
    tokens.iter().any(|t| candidates.contains(t))
}

fn matches_error_intent(lower: &str, tokens: &[&str]) -> bool {
    token_matches(tokens, &["error", "errors", "err", "fault", "failure"])
        && token_matches(
            tokens,
            &[
                "propagated",
                "propagate",
                "propagates",
                "handled",
                "handle",
                "handles",
                "produced",
                "produce",
                "produces",
                "thrown",
                "throw",
                "throws",
                "raised",
                "raise",
                "raises",
                "caught",
                "catch",
                "catches",
                "managed",
                "manage",
                "manages",
                "returned",
                "return",
                "returns",
                "created",
                "create",
                "creates",
            ],
        )
        || lower.contains("error propagation")
        || lower.contains("error handling")
        || lower.contains("error flow")
        || lower.contains("error chain")
        || (lower.contains("fail") && token_matches(tokens, &["on", "when"]))
        || (lower.contains("what happens when") && token_matches(tokens, &["error", "fail"]))
}

fn matches_schedule_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "scheduled",
            "schedule",
            "scheduling",
            "dispatched",
            "dispatch",
            "queued",
            "queue",
        ],
    )
}

fn matches_create_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "created",
            "create",
            "constructed",
            "construct",
            "instantiated",
            "instantiate",
            "spawned",
            "spawn",
            "allocated",
            "allocate",
        ],
    ) && !token_matches(tokens, &["error", "errors"])
}

fn matches_validate_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "validated",
            "validate",
            "validates",
            "validation",
            "verified",
            "verify",
            "verifies",
            "checked",
            "check",
            "checks",
        ],
    )
}

fn matches_transform_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "transformed",
            "transform",
            "converted",
            "convert",
            "mapped",
            "map",
            "translated",
            "translate",
            "adapted",
            "adapt",
            "composed",
            "compose",
        ],
    )
}

fn matches_io_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "read",
            "written",
            "write",
            "sent",
            "send",
            "received",
            "receive",
            "connected",
            "connect",
            "loaded",
            "load",
            "saved",
            "save",
            "persisted",
            "persist",
            "stored",
            "store",
            "fetched",
            "fetch",
            "network",
            "socket",
            "stream",
            "file",
            "disk",
            "database",
            "db",
        ],
    )
}

fn matches_init_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "initialized",
            "initialize",
            "init",
            "bootstrapped",
            "bootstrap",
            "started",
            "start",
            "launched",
            "launch",
            "setup",
        ],
    )
}

fn matches_cleanup_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "cleaned", "cleanup", "clean", "teardown", "shutdown", "shut", "disposed", "dispose",
            "released", "release", "freed", "free", "dropped", "drop",
        ],
    )
}

fn matches_route_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "routed",
            "route",
            "routing",
            "dispatched",
            "dispatch",
            "endpoint",
            "endpoints",
            "request",
            "requests",
        ],
    )
}

fn matches_auth_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "authenticated",
            "authenticate",
            "auth",
            "authentication",
            "authorized",
            "authorize",
            "authorization",
            "permission",
            "permissions",
            "token",
            "tokens",
            "credential",
            "credentials",
            "login",
            "logout",
        ],
    )
}

fn matches_parse_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "parsed",
            "parse",
            "parsing",
            "lexed",
            "lex",
            "tokenized",
            "tokenize",
            "scanned",
            "scan",
        ],
    )
}

fn matches_serialize_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "serialized",
            "serialize",
            "deserialized",
            "deserialize",
            "encoded",
            "encode",
            "decoded",
            "decode",
            "marshaled",
            "marshal",
        ],
    )
}

fn matches_cache_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &["cached", "cache", "caching", "memoized", "memoize", "lru"],
    )
}

fn matches_log_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "logged",
            "log",
            "logging",
            "traced",
            "trace",
            "debugged",
            "debug",
            "monitored",
            "monitor",
            "recorded",
            "record",
        ],
    )
}

fn matches_event_intent(_lower: &str, tokens: &[&str]) -> bool {
    token_matches(
        tokens,
        &[
            "event",
            "events",
            "emitted",
            "emit",
            "published",
            "publish",
            "subscribed",
            "subscribe",
            "notified",
            "notify",
            "observed",
            "observe",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_propagation_query() {
        let roles = query_to_expected_roles("how are errors propagated");
        assert!(roles.contains(&RoleTag::ErrorProducer));
        assert!(roles.contains(&RoleTag::ErrorPropagator));
        assert!(roles.contains(&RoleTag::ErrorHandler));
    }

    #[test]
    fn test_error_handling_query() {
        let roles = query_to_expected_roles("what handles errors in the system");
        assert!(roles.contains(&RoleTag::ErrorHandler));
    }

    #[test]
    fn test_create_query() {
        let roles = query_to_expected_roles("where is a connection created");
        assert!(roles.contains(&RoleTag::Factory));
        assert!(roles.contains(&RoleTag::Builder));
    }

    #[test]
    fn test_schedule_query() {
        let roles = query_to_expected_roles("how are tasks scheduled");
        assert!(roles.contains(&RoleTag::Scheduler));
    }

    #[test]
    fn test_validate_query() {
        let roles = query_to_expected_roles("what validates input");
        assert!(roles.contains(&RoleTag::Validator));
    }

    #[test]
    fn test_io_query() {
        let roles = query_to_expected_roles("how is data saved to disk");
        assert!(roles.contains(&RoleTag::IO));
    }

    #[test]
    fn test_no_roles_for_code_query() {
        let roles = query_to_expected_roles("RateLimiter");
        assert!(roles.is_empty());
    }

    #[test]
    fn test_role_boost_fts() {
        let boosted = role_boost_fts_query(
            "how are errors propagated",
            &[RoleTag::ErrorProducer, RoleTag::ErrorPropagator],
        );
        assert!(boosted.contains("error"));
        assert!(boosted.len() > "how are errors propagated".len());
    }

    #[test]
    fn test_is_role_query() {
        assert!(is_role_query("how are errors propagated"));
        assert!(is_role_query("what validates input"));
        assert!(is_role_query("where is data stored"));
        assert!(!is_role_query("RateLimiter"));
        assert!(!is_role_query("cache"));
    }

    #[test]
    fn test_auth_query() {
        let roles = query_to_expected_roles("how is authentication handled");
        assert!(roles.contains(&RoleTag::AuthGate));
        assert!(roles.contains(&RoleTag::Guard));
    }

    #[test]
    fn test_parse_query() {
        let roles = query_to_expected_roles("how is source code parsed");
        assert!(roles.contains(&RoleTag::Parser));
    }

    #[test]
    fn test_transform_query() {
        let roles = query_to_expected_roles("how is data transformed");
        assert!(roles.contains(&RoleTag::Transformer));
        assert!(roles.contains(&RoleTag::Transform));
    }
}
