# MCP Tools

GraphIQ ships an MCP server (`graphiq-mcp`) that agent harnesses talk to over stdio.

## Running the server

```bash
graphiq-mcp /path/to/project --watch
```

The server auto-binds supported provider workspace roots, lazily builds its index on first search, and detects/recreates corrupted or wrong-project databases automatically. In session/background mode (`--ephemeral` or `--session-id`) the default caps CPU to 4 threads so a harness-spawned daemon stays light.

## Tools

| Tool | Use |
|---|---|
| `briefing` | Project overview and starting context |
| `search` | Ranked symbol search with structural scoring |
| `context` | Source plus graph neighborhood for a symbol |
| `blast` | Forward or backward impact radius |
| `impact` | Git diff impact report |
| `interrogate` | Structural architecture questions |
| `topology` | Local graph and subsystem structure |
| `constants` | Numeric and string constant lookup |
| `explain` / `why` | Why a symbol ranks where it does |
| `status` / `doctor` / `upgrade_index` | Index health, diagnosis, rebuild |
| `index` / `clear` | Full reindex / wipe to empty |

## Search output

`search` returns a compact Markdown result list by default. Each result includes
the symbol, kind, file and line range, score, and small structural tags; a
signature or one-line doc comment is included when available. This keeps the
common exploration path readable and bounded for agent context windows.

```text
## Search results for `rate limit middleware`
4 matches · family: NaturalDescriptive · mode: GraphWalk · format: compact

1. **RateLimiter** · class · src/auth/rate_limit.rs:18-74 · score 8.52 · hub
   `pub struct RateLimiter { ... }`
```

Use `format: "detailed"` when source previews and caller/callee lists are
actually needed. Use `context` for one selected implementation; when a name
is ambiguous, pass the result's path as `file_filter`:

```json
{"symbol":"RateLimiter","file_filter":"src/auth/rate_limit.rs"}
```

Search still accepts `top_k` (default 10, maximum 50), `file_filter`, and
`cluster` (default true). Clustering retrieves a wider candidate pool, then
spreads results across files while preserving the underlying scores.

## Tool naming across surfaces

| Surface | Tool names |
|---|---|
| **Native** (`graphiq-mcp`) | unprefixed — `search`, `context`, `blast`, `briefing`, `status`, `clear`, … |
| **Signet** | `signet_code_` prefixed — `signet_code_search`, `signet_code_context`, … (with unprefixed aliases) |
| **Pi extension** | `graphiq_` prefixed — `graphiq_search`, `graphiq_context`, `graphiq_status` |

See [Supported harnesses](supported-harnesses.md) for how each harness connects.
