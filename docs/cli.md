# CLI Reference

`graphiq --help` shows every flag. The full command guide for agents is in [the skill](../skills/graphiq/SKILL.md) and the [AGENTS.md template](../crates/graphiq-cli/AGENTS.md.template).

## Commands

| Command | What it does |
|---|---|
| `index <path>` | Index a project (incremental by content hash) |
| `search <query>` | Ranked symbol search (name, natural language, file path, or error message) |
| `context <symbol>` | Read a symbol's source + structural neighborhood |
| `blast <symbol>` | Trace change impact (forward/backward radius) |
| `impact --project <p>` | Analyze current git changes for affected symbols |
| `briefing [--compact]` | Get oriented: architecture, subsystems, public API, hubs |
| `interrogate` | Ask structural questions (entry points, error boundaries, coupling) |
| `topology <symbol>` | Map the local graph neighborhood |
| `constants [query]` | Find shared numeric/string constants |
| `dead-code` | Find unreachable symbols |
| `status` / `doctor` / `upgrade-index` | Index health, diagnosis, rebuild artifacts |
| `clear` | Delete the index, leave a fresh empty database |

### Lifecycle & harness commands

| Command | What it does |
|---|---|
| `setup` | Wire graphiq into agent harnesses |
| `sync [--apply]` | Verify harness attach + write the registry; `--apply` re-configures missing ones |
| `discover` | Scan the system for installed agent harnesses |
| `git` | Current project scope: branch, working tree, recent commits |
| `projects [--forget <path\|all>]` | List tracked projects + their index health |

## Common flags

- `--db <path>` — use a specific index database (default `.graphiq/graphiq.db`)
- `--top <n>` / `--depth <n>` — result count / traversal depth
- `--json` — machine-readable output (for `discover`, `git`, `projects`, `impact`)
- `--yes` — skip confirmation prompts (`clear`)
