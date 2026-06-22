# GraphIQ

**Local code intelligence for agents and developers.** GraphIQ indexes a repository into a structural graph of symbols, calls, imports, constants, and type flow — then searches it with ranked retrieval that understands how code is connected, not just what strings it contains. No embeddings, no LLM, no network calls. Everything lives in a single SQLite file.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/graphiq-logo-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/graphiq-logo-light.png">
    <img src="docs/assets/graphiq-logo-light.png" alt="GraphIQ" width="132">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/aaf2tbz/graphiq/releases"><img src="https://img.shields.io/github/v/release/aaf2tbz/graphiq?include_prereleases&style=for-the-badge" alt="GitHub release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge" alt="MIT License"></a>
  <a href="docs/performance.md"><img src="https://img.shields.io/badge/NDCG%4010%20%2B48%25%20%7C%20MRR%4010%20%2B128%25-vs%20grep-black?style=for-the-badge" alt="Benchmark signal"></a>
  <a href="https://github.com/aaf2tbz/graphiq/actions/workflows/linux-smoke.yml"><img src="https://img.shields.io/github/actions/workflow/status/aaf2tbz/graphiq/linux-smoke.yml?branch=main&label=linux&style=for-the-badge" alt="Linux smoke"></a>
</p>

---

## Documentation

### Start here

- **[Install](docs/install.md)** — Homebrew, install script, or build from source
- **[Quickstart](docs/quickstart.md)** — index, search, wire into a harness
- **[How it works](docs/how-graphiq-works.md)** — the indexing + search pipeline
- **[What gets indexed](docs/indexing.md)** — languages, symbol types, graph edges

### Using GraphIQ

- **[CLI reference](docs/cli.md)** — every command and flag
- **[MCP tools](docs/mcp.md)** — the tool surface agents see
- **[Supported harnesses](docs/supported-harnesses.md)** — Claude Code, Codex, OpenCode, Cursor, Pi, and more
- **[Desktop app](docs/desktop.md)** — the visual index browser

### Reliability & performance

- **[Reliability & resource control](docs/reliability.md)** — CPU caps, dormancy, crash-safety
- **[Performance & benchmarks](docs/performance.md)** — measured quality vs grep

### Going deeper

- **[Benchmarks](docs/benchmarks.md)** — full methodology and per-codebase results
- **[Signet plugin](docs/signet-plugin.md)** — the managed plugin for Signet users
- **[Research notes](docs/research.md)** — 29 phases of experimentation
- **[Hardening roadmap](docs/hardening-roadmap.md)** — reliability work, past and planned
- **[Contributing](docs/contributing.md)** — building, testing, and CI

## Quick example

```bash
graphiq index /path/to/project
graphiq search "rate limit middleware"
```

See **[Quickstart](docs/quickstart.md)** for the full flow, including wiring GraphIQ into your AI coding agent.

## License

MIT — see [LICENSE](LICENSE).
