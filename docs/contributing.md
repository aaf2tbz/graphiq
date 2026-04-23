# Contributing to GraphIQ

## Setup

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

## Architecture

GraphIQ is a Rust workspace with these crates:

- `graphiq-core` — Indexing, search, and analysis engine
- `graphiq-cli` — Command-line interface
- `graphiq-mcp` — MCP server (experimental)
- `graphiq-bench` — Benchmarks

## Development workflow

1. Create a feature branch from `main`.
2. Make changes with tests.
3. Run `cargo test`, `cargo clippy`, `cargo fmt --check`.
4. Open a pull request against `main`.

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

```bash
cargo bench
```

Results are saved to `benchmarks/`.

## Release

**Releases are managed exclusively by Alex Mondello (`@aaf2tbz`).** Do not tag releases, bump versions, or publish formula updates on your own. If you believe a release is needed, open an issue or comment on an existing PR.

The release process (for maintainers):

1. Bump versions in `Cargo.toml` files and `signet-plugin/manifest.json`.
2. Tag with `vX.Y.Z`.
3. Push tag — CI builds and publishes the Homebrew formula.
