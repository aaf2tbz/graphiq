
pub struct BehavioralDescriptor {
    pub phrases: Vec<String>,
}

pub fn generate_behavioral_descriptors(
    name: &str,
    _kind: &str,
    signature: Option<&str>,
    source: Option<&str>,
    callee_names: &[String],
    caller_names: &[String],
    file_path: Option<&str>,
) -> BehavioralDescriptor {
    let mut phrases: Vec<String> = Vec::new();

    let name_dec: Vec<&str> = name
        .split(|c: char| c == '_' || c.is_uppercase())
        .filter(|w| !w.is_empty())
        .collect();
    let name_lower = name.to_lowercase();

    let verb = infer_behavioral_verb(&name_dec, &name_lower, callee_names, signature);

    let object = infer_object(
        &name_dec,
        &name_lower,
        callee_names,
        signature,
        source,
        file_path,
    );

    let context = infer_context(callee_names, caller_names, file_path);

    if !verb.is_empty() && !object.is_empty() {
        phrases.push(format!("{} {}", verb, object));
    } else if !verb.is_empty() {
        phrases.push(verb);
    }

    if !context.is_empty() {
        phrases.push(context);
    }

    BehavioralDescriptor { phrases }
}

fn infer_behavioral_verb(
    name_dec: &[&str],
    name_lower: &str,
    callees: &[String],
    _sig: Option<&str>,
) -> String {
    let name_verbs: &[(&str, &str)] = &[
        ("get", "gets retrieves fetches"),
        ("set", "sets configures updates"),
        ("check", "checks validates verifies"),
        ("validate", "validates verifies checks"),
        ("verify", "verifies validates checks"),
        ("is", "determines checks tests"),
        ("has", "checks tests determines"),
        ("can", "determines checks tests"),
        ("should", "determines checks tests"),
        ("create", "creates builds constructs"),
        ("build", "builds creates generates"),
        ("make", "creates builds generates"),
        ("new", "creates initializes constructs"),
        ("init", "initializes creates sets up"),
        ("start", "starts launches boots"),
        ("stop", "stops shuts down halts"),
        ("restart", "restarts reboots reloads"),
        ("run", "runs executes performs"),
        ("exec", "executes runs performs"),
        ("parse", "parses extracts reads"),
        ("extract", "extracts parses reads"),
        ("read", "reads loads fetches"),
        ("load", "loads reads fetches"),
        ("write", "writes saves persists"),
        ("save", "saves writes persists"),
        ("store", "stores saves persists"),
        ("insert", "inserts creates adds"),
        ("delete", "deletes removes drops"),
        ("remove", "removes deletes drops"),
        ("prune", "prunes removes cleans"),
        ("clean", "cleans removes clears"),
        ("clear", "clears removes resets"),
        ("reset", "resets clears restores"),
        ("update", "updates modifies changes"),
        ("modify", "modifies updates changes"),
        ("transform", "transforms converts maps"),
        ("convert", "converts transforms maps"),
        ("map", "maps transforms converts"),
        ("encode", "encodes serializes formats"),
        ("decode", "decodes deserializes parses"),
        ("serialize", "serializes encodes formats"),
        ("format", "formats encodes structures"),
        ("search", "searches queries finds"),
        ("query", "queries searches finds"),
        ("find", "finds searches locates"),
        ("locate", "locates finds discovers"),
        ("fetch", "fetches retrieves loads"),
        ("send", "sends transmits dispatches"),
        ("receive", "receives accepts handles"),
        ("handle", "handles processes manages"),
        ("process", "processes handles transforms"),
        ("manage", "manages controls administers"),
        ("control", "controls manages governs"),
        ("guard", "guards protects enforces"),
        ("protect", "protects guards secures"),
        ("secure", "secures protects guards"),
        ("auth", "authenticates authorizes verifies"),
        ("login", "authenticates logs in"),
        ("register", "registers creates signs up"),
        ("subscribe", "subscribes listens watches"),
        ("listen", "listens waits subscribes"),
        ("watch", "watches observes monitors"),
        ("observe", "observes monitors tracks"),
        ("monitor", "monitors observes tracks"),
        ("track", "tracks monitors records"),
        ("log", "logs records tracks"),
        ("record", "records logs saves"),
        ("emit", "emits publishes sends"),
        ("publish", "publishes emits broadcasts"),
        ("broadcast", "broadcasts publishes sends"),
        ("notify", "notifies signals alerts"),
        ("alert", "alerts notifies warns"),
        ("warn", "warns alerts logs"),
        ("error", "handles reports errors"),
        ("throw", "throws raises signals"),
        ("catch", "catches handles intercepts"),
        ("retry", "retries attempts again"),
        ("schedule", "schedules queues plans"),
        ("queue", "queues schedules buffers"),
        ("buffer", "buffers caches queues"),
        ("cache", "caches stores buffers"),
        ("embed", "embeds encodes vectorizes"),
        ("vector", "vectorizes converts embeds"),
        ("recall", "recalls retrieves searches"),
        ("remember", "remembers stores saves"),
        ("forget", "forgets removes deletes"),
        ("resolve", "resolves determines finds"),
        ("determine", "determines computes decides"),
        ("compute", "computes calculates evaluates"),
        ("evaluate", "evaluates computes assesses"),
        ("assess", "assesses evaluates checks"),
        ("score", "scores ranks evaluates"),
        ("rank", "ranks scores orders"),
        ("sort", "sorts orders ranks"),
        ("order", "orders sorts arranges"),
        ("filter", "filters selects excludes"),
        ("select", "selects filters picks"),
        ("pick", "selects chooses picks"),
        ("choose", "chooses selects picks"),
        ("merge", "merges combines joins"),
        ("combine", "combines merges joins"),
        ("join", "joins connects links"),
        ("split", "splits separates divides"),
        ("divide", "divides separates splits"),
        ("group", "groups clusters aggregates"),
        ("aggregate", "aggregates groups collects"),
        ("collect", "collects aggregates gathers"),
        ("count", "counts tallies measures"),
        ("measure", "measures counts quantifies"),
        ("compare", "compares evaluates diffs"),
        ("diff", "diffs compares changes"),
        ("clone", "clones copies duplicates"),
        ("copy", "copies clones duplicates"),
        ("sync", "syncs synchronizes aligns"),
        ("migrate", "migrates transforms converts"),
        ("upgrade", "upgrades updates migrates"),
        ("install", "installs sets up deploys"),
        ("deploy", "deploys installs publishes"),
        ("connect", "connects links joins"),
        ("disconnect", "disconnects closes unlinks"),
        ("bind", "binds attaches connects"),
        ("unbind", "unbinds detaches disconnects"),
        ("attach", "attaches binds connects"),
        ("detach", "detaches unbinds disconnects"),
        ("scope", "scopes restricts limits"),
        ("restrict", "restricts limits scopes"),
        ("limit", "limits restricts caps"),
        ("cap", "caps limits restricts"),
        ("health", "checks monitors determines"),
        ("status", "checks reports determines"),
        ("diagnose", "diagnoses checks analyzes"),
        ("analyze", "analyzes examines inspects"),
        ("inspect", "inspects examines checks"),
        ("test", "tests validates checks"),
        ("assert", "asserts validates checks"),
        ("expect", "expects validates checks"),
        ("mock", "mocks simulates fakes"),
        ("stub", "stubs fakes replaces"),
        ("fake", "fakes mocks simulates"),
        ("setup", "sets up initializes configures"),
        ("teardown", "tears down cleans destroys"),
        ("cleanup", "cleans up removes resets"),
    ];

    for (stem, verbs) in name_verbs {
        if name_dec.iter().any(|w| w.eq_ignore_ascii_case(stem)) {
            return verbs.split_whitespace().next().unwrap_or("").to_string();
        }
    }

    if name_lower.contains("manager") {
        return "manages".into();
    }
    if name_lower.contains("controller") {
        return "controls".into();
    }
    if name_lower.contains("handler") {
        return "handles".into();
    }
    if name_lower.contains("provider") {
        return "provides supplies".into();
    }
    if name_lower.contains("factory") {
        return "creates constructs".into();
    }
    if name_lower.contains("builder") {
        return "builds constructs".into();
    }
    if name_lower.contains("config") {
        return "configures sets up".into();
    }
    if name_lower.contains("registry") {
        return "registers tracks manages".into();
    }
    if name_lower.contains("store") {
        return "stores persists saves".into();
    }
    if name_lower.contains("cache") {
        return "caches buffers stores".into();
    }

    if !callees.is_empty() {
        let callee_verbs: Vec<&str> = callees
            .iter()
            .take(5)
            .filter_map(|c| {
                let cl = c.to_lowercase();
                for (stem, _) in name_verbs.iter().take(20) {
                    if cl.contains(stem) {
                        return Some(*stem);
                    }
                }
                None
            })
            .collect();
        if !callee_verbs.is_empty() {
            return callee_verbs[0].to_string();
        }
    }

    String::new()
}

fn infer_object(
    name_dec: &[&str],
    _name_lower: &str,
    callees: &[String],
    sig: Option<&str>,
    source: Option<&str>,
    file_path: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    let domain_words: &[&str] = &[
        "token",
        "permission",
        "scope",
        "role",
        "auth",
        "session",
        "memory",
        "embedding",
        "vector",
        "search",
        "query",
        "recall",
        "daemon",
        "connector",
        "health",
        "pipeline",
        "extraction",
        "entity",
        "dependency",
        "plugin",
        "mcp",
        "tool",
        "provider",
        "config",
        "file",
        "path",
        "module",
        "channel",
        "stream",
        "buffer",
        "cache",
        "lock",
        "mutex",
        "semaphore",
        "task",
        "runtime",
        "scheduler",
        "timer",
        "interval",
        "deadline",
        "tcp",
        "udp",
        "socket",
        "listener",
        "acceptor",
        "bundle",
        "module",
        "import",
        "export",
        "loader",
        "resolver",
        "source",
        "map",
        "css",
        "tree",
        "shake",
        "unused",
        "symbol",
    ];

    for word in name_dec {
        let wl = word.to_lowercase();
        if domain_words.contains(&wl.as_str()) {
            parts.push(wl);
        }
    }

    if let Some(path) = file_path {
        let path_lower = path.to_lowercase();
        if path_lower.contains("auth") && !parts.contains(&"auth".into()) {
            parts.push("auth".into());
        }
        if path_lower.contains("permission") && !parts.contains(&"permission".into()) {
            parts.push("permission".into());
        }
        if path_lower.contains("token") && !parts.contains(&"token".into()) {
            parts.push("token".into());
        }
        if path_lower.contains("session") && !parts.contains(&"session".into()) {
            parts.push("session".into());
        }
        if path_lower.contains("memory") && !parts.contains(&"memory".into()) {
            parts.push("memory".into());
        }
        if path_lower.contains("daemon") && !parts.contains(&"daemon".into()) {
            parts.push("daemon".into());
        }
        if path_lower.contains("connector") && !parts.contains(&"connector".into()) {
            parts.push("connector".into());
        }
        if path_lower.contains("embed") && !parts.iter().any(|p| p == "embedding") {
            parts.push("embedding".into());
        }
        if path_lower.contains("search") && !parts.contains(&"search".into()) {
            parts.push("search".into());
        }
        if path_lower.contains("health") && !parts.contains(&"health".into()) {
            parts.push("health".into());
        }
        if path_lower.contains("pipeline") && !parts.contains(&"pipeline".into()) {
            parts.push("pipeline".into());
        }
        if path_lower.contains("extract") && !parts.iter().any(|p| p == "extraction") {
            parts.push("extraction".into());
        }
        if path_lower.contains("scope") && !parts.contains(&"scope".into()) {
            parts.push("scope".into());
        }
    }

    if let Some(src) = source {
        let src_lower = src.to_lowercase();
        let src_domain: &[&str] = &[
            "jwt",
            "oauth",
            "bearer",
            "api",
            "key",
            "secret",
            "credential",
            "postgres",
            "sqlite",
            "redis",
            "database",
            "schema",
            "migration",
            "typescript",
            "javascript",
            "rust",
            "python",
            "go",
        ];
        for word in src_domain {
            if src_lower.contains(word) && !parts.iter().any(|p| p.contains(word)) {
                parts.push(word.to_string());
            }
        }
    }

    for callee in callees.iter().take(8) {
        let cl = callee.to_lowercase();
        for word in domain_words {
            if cl.contains(word) && !parts.iter().any(|p| p == word) {
                parts.push(word.to_string());
            }
        }
    }

    if let Some(s) = sig {
        let sig_lower = s.to_lowercase();
        let sig_types: &[&str] = &[
            "token",
            "permission",
            "scope",
            "role",
            "session",
            "memory",
            "embedding",
            "connector",
            "daemon",
            "health",
            "pipeline",
            "channel",
            "stream",
            "task",
            "timer",
            "interval",
            "bundle",
            "plugin",
            "source",
            "map",
            "css",
        ];
        for word in sig_types {
            if sig_lower.contains(word) && !parts.iter().any(|p| p == word) {
                parts.push(word.to_string());
            }
        }
    }

    parts.dedup();
    parts.truncate(4);
    parts.join(" ")
}

fn infer_context(callees: &[String], callers: &[String], file_path: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();

    let action_patterns: &[(&[&str], &str)] = &[
        (
            &["require", "permission", "check", "scope"],
            "permission authorization gate",
        ),
        (&["validate", "verify", "assert"], "validation verification"),
        (
            &["error", "fail", "reject", "throw"],
            "error handling failure path",
        ),
        (
            &["parse", "extract", "transform", "convert"],
            "data processing pipeline",
        ),
        (
            &["create", "build", "make", "construct", "new"],
            "creation initialization",
        ),
        (&["delete", "remove", "drop", "prune"], "deletion cleanup"),
        (
            &["update", "modify", "change", "set"],
            "modification update",
        ),
        (
            &["search", "query", "find", "locate", "recall"],
            "search retrieval lookup",
        ),
        (&["embed", "vector", "encode"], "embedding vectorization"),
        (
            &["start", "launch", "boot", "spawn", "init"],
            "startup initialization",
        ),
        (
            &["stop", "shutdown", "halt", "kill", "terminate"],
            "shutdown teardown",
        ),
        (&["restart", "reboot", "reload"], "restart recovery"),
        (&["connect", "bind", "attach"], "connection binding"),
        (&["disconnect", "unbind", "detach"], "disconnection cleanup"),
        (
            &["send", "emit", "publish", "notify"],
            "output dispatch notification",
        ),
        (
            &["receive", "listen", "watch", "subscribe"],
            "input reception observation",
        ),
        (
            &["cache", "buffer", "store", "save"],
            "caching storage persistence",
        ),
        (
            &["schedule", "queue", "debounce", "throttle"],
            "scheduling rate limiting",
        ),
        (
            &["auth", "login", "token", "jwt", "session"],
            "authentication session management",
        ),
        (&["health", "status", "diagnos"], "health check monitoring"),
        (
            &["daemon", "process", "service"],
            "process daemon management",
        ),
        (
            &["tree", "shake", "dead", "code", "unused"],
            "tree shaking dead code elimination",
        ),
        (
            &["plugin", "resolve", "load", "on_resolve", "on_load"],
            "plugin resolution loading",
        ),
        (&["source", "map", "sourcemap"], "source map generation"),
        (&["css", "prefix", "minif"], "css processing prefixing"),
        (
            &["bundle", "output", "generate", "emit"],
            "bundling code generation",
        ),
    ];

    let all_related: Vec<String> = callees
        .iter()
        .chain(callers.iter())
        .take(12)
        .map(|n| n.to_lowercase())
        .collect();

    for (keywords, label) in action_patterns {
        if all_related
            .iter()
            .any(|name| keywords.iter().any(|kw| name.contains(kw)))
        {
            parts.push(label.to_string());
            break;
        }
    }

    if let Some(path) = file_path {
        let path_lower = path.to_lowercase();
        if path_lower.contains("test") || path_lower.contains("spec") {
            parts.push("test".into());
        }
    }

    parts.join(". ")
}
