# Install

GraphIQ runs on macOS (Apple Silicon + Intel), Linux (x86_64 + aarch64), and Windows (builds from source).

## Homebrew

```bash
brew tap aaf2tbz/graphiq
brew install graphiq
```

## Install Script (macOS & Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
```

The script downloads a prebuilt binary for your platform, verifies the SHA-256 checksum, and places `graphiq` + `graphiq-mcp` on your PATH. On Linux it detects a missing Vulkan loader and offers to install one (optional — GraphIQ runs on CPU without it).

## From Source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

For GPU acceleration add `--features gpu` (requires a Vulkan-capable GPU + loader).

## Requirements

- Rust 1.75+ (stable)
- A C compiler (for the tree-sitter grammar builds)
- `pkg-config`
- SQLite is bundled.

## Platform Support

| Platform | Status | Notes |
|---|---|---|
| **macOS** (Apple Silicon) | ✅ Primary | prebuilt release |
| **macOS** (Intel) | ✅ Supported | prebuilt release |
| **Linux** (x86_64) | ✅ Supported | prebuilt release; [smoke-tested in CI](https://github.com/aaf2tbz/graphiq/actions/workflows/linux-smoke.yml) with zero Vulkan installed |
| **Linux** (aarch64) | ✅ Built | prebuilt release; built + RPATH-asserted in CI |
| **Windows** | ⚠️ Builds | not prebuilt; `which`/path checks fall back to `where` |

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
```

This removes `graphiq`, `graphiq-mcp`, and `graphiq-bench`. Project-local `.graphiq/` indexes are left in place (delete them manually if desired).
