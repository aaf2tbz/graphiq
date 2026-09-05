# Install

GraphIQ runs on macOS (Apple Silicon + Intel), Linux (x86_64 + aarch64), and Windows (x86_64).

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

## Windows (PowerShell)

```powershell
$installer = Join-Path $env:TEMP "graphiq-install.ps1"
Invoke-WebRequest https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.ps1 -OutFile $installer
powershell -ExecutionPolicy Bypass -File $installer
Remove-Item $installer
```

The installer downloads the `x86_64-pc-windows-msvc` release, verifies its
SHA-256 checksum, installs `graphiq.exe`, `graphiq-mcp.exe`, and
`graphiq-bench.exe` under `%LOCALAPPDATA%\GraphIQ\bin`, and adds that directory
to the current user's PATH. Open a new terminal after installation. To remove
it, run the same script with `-Uninstall`.

## From Source

```bash
git clone https://github.com/aaf2tbz/graphiq.git
cd graphiq
cargo build --release
```

For GPU acceleration add `--features gpu`. On macOS this selects the system Metal backend (no Vulkan or extra runtime is required); on Linux it uses Vulkan when a compatible loader/GPU is available; on Windows it uses DirectX 12 when available. Every platform falls back to CPU when GPU initialization is unavailable. Indexing and large search score batches use the GPU automatically; small searches stay on CPU to avoid transfer overhead. Set `GRAPHIQ_DISABLE_GPU=1` to force the fallback path.

## Requirements

- Rust 1.75+ (stable)
- A C compiler (for the tree-sitter grammar builds)
- `pkg-config`
- SQLite is bundled.

Prebuilt Windows releases do not require Rust, a C compiler, or `pkg-config`.
Windows source builds require the Visual Studio C++ build tools.

## Platform Support

| Platform | Status | Notes |
|---|---|---|
| **macOS** (Apple Silicon) | ✅ Primary | prebuilt release; Metal acceleration on macOS 11+ |
| **macOS** (Intel) | ✅ Supported | prebuilt release; Metal acceleration depends on the OS/GPU |
| **Linux** (x86_64) | ✅ Supported | prebuilt release; [smoke-tested in CI](https://github.com/aaf2tbz/graphiq/actions/workflows/linux-smoke.yml) with zero Vulkan installed |
| **Linux** (aarch64) | ✅ Built | prebuilt release; built + RPATH-asserted in CI |
| **Windows** | ✅ Supported | prebuilt x86_64 release; DirectX 12 when available, CPU fallback otherwise |

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
```

This removes `graphiq`, `graphiq-mcp`, and `graphiq-bench`. Project-local `.graphiq/` indexes are left in place (delete them manually if desired).

On Windows:

```powershell
$installer = Join-Path $env:TEMP "graphiq-install.ps1"
Invoke-WebRequest https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.ps1 -OutFile $installer
powershell -ExecutionPolicy Bypass -File $installer -Uninstall
Remove-Item $installer
```
