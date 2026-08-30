# Contributing to GraphIQ

## Setup

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

## Architecture

GraphIQ is a Rust workspace. The layout:

| Crate / dir | Role |
|---|---|
| `graphiq-core` | Indexing, search, and analysis engine (the heart) |
| `graphiq-cli` | Command-line interface → `graphiq` binary |
| `graphiq-mcp` | MCP stdio server → `graphiq-mcp` binary |
| `graphiq-bench` | Benchmark harness |
| `signet-plugin/` | Signet managed-plugin manifest + session hooks |
| `integrations/pi/` | pi extension (no MCP — shells out to the CLI) |

The CLI surfaces index/search/blast plus lifecycle commands: `setup`, `sync`
(--apply re-configures), `discover`, `git`, `projects`, `clear`. See the
[CLI reference in the README](../README.md#cli-reference) and the
[AGENTS.md template](../crates/graphiq-cli/AGENTS.md.template) for the full set.

## Development workflow

1. Create a feature branch from `main`.
2. Make changes with tests (prefer pure, unit-testable helpers).
3. Validate locally:
   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets
   cargo fmt --all -- --check
   ```
4. For harness/integration changes, rebuild and **probe the real binary** — don't
   rely on unit tests alone. See the verification patterns in recent commit
   messages.
5. Open a pull request against `main`.

## CI

- **`linux-smoke.yml`** — builds with `--features gpu` on ubuntu, strips all
  Vulkan, and proves the binaries start + run the full command surface on CPU.
  This is the gate that answers "does it work on Linux".
- **`release.yml`** — cross-builds macOS (arm64 + x86_64) and Linux (x86_64 +
  aarch64) releases with SHA-256 checksums.
- **`auto-release.yml`** — tags the next version on every push to `main`.

## Commit style

Use conventional commits:

- `feat:` for new features
- `fix:` for bug fixes
- `refactor:` for internal changes
- `docs:` for documentation
- `perf:` for performance improvements
- `chore:` for maintenance

## Signet plugin

The Signet plugin manifest lives at `signet-plugin/manifest.json`. Update it when:

- Adding or changing MCP tool names
- Adding connector capabilities for new harnesses
- Changing CLI commands exposed to Signet
- Updating prompt guidance text

After manifest changes, rebuild the Signet daemon if testing locally:

```bash
cd signetai
bun run build
```

## Benchmarks

The benchmark harness compares GraphIQ against grep on real query sets:

```bash
# Build a DB, then:
./target/release/graphiq-bench <db-path> <ndcg-queries.json> <mrr-queries.json>
```

Query sets live in `benches/queries/archive/`. When changing the indexer or
scoring, run an **A/B** against `main` to prove no regression (the demo-codebase
MRR suite is a good quick check). Baselines are in `benches/baseline-*.json`.

## Release

**Releases are managed exclusively by Alex Mondello (`@aaf2tbz`).** Do not tag releases, bump versions, or publish formula updates on your own. If you believe a release is needed, open an issue or comment on an existing PR.

The release process (for maintainers):

1. Bump versions in `Cargo.toml` files and `signet-plugin/manifest.json`.
2. Tag with `vX.Y.Z`.
3. Push tag — CI builds and publishes the Homebrew formula.
