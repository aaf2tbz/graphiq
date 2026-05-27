#!/usr/bin/env bash
# GraphIQ install/uninstall script
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
#
# Environment variables:
#   GRAPHIQ_INSTALL_DIR  — installation directory (default: /usr/local/bin)
#   GRAPHIQ_VERSION      — specific version to install (default: latest)
set -euo pipefail

REPO="aaf2tbz/graphiq"
INSTALL_DIR="${GRAPHIQ_INSTALL_DIR:-/usr/local/bin}"
COMMAND="${1:-install}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { echo "${GREEN}  ✓${RESET} $*"; }
warn()  { echo "${YELLOW}  ⚠${RESET} $*"; }
error() { echo "${RED}  ✗${RESET} $*" >&2; }

confirm() {
    printf "  %s [y/N] " "$1"
    read -r reply
    [ "$reply" = "y" ] || [ "$reply" = "Y" ] || [ "$reply" = "yes" ]
}

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        error "requires '$1' but it is not installed"
        if [ -n "${2:-}" ]; then
            echo "  Install: $2" >&2
        fi
        exit 1
    fi
}

detect_platform() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    ARCH="$(uname -m)"

    case "$OS" in
        darwin)
            if [ "$ARCH" = "arm64" ]; then
                echo "aarch64-apple-darwin"
            elif [ "$ARCH" = "x86_64" ]; then
                echo "x86_64-apple-darwin"
            else
                error "unsupported macOS architecture: $ARCH"
                exit 1
            fi
            ;;
        linux)
            if [ "$ARCH" = "x86_64" ]; then
                echo "x86_64-unknown-linux-gnu"
            elif [ "$ARCH" = "aarch64" ]; then
                echo "aarch64-unknown-linux-gnu"
            else
                error "unsupported Linux architecture: $ARCH"
                exit 1
            fi
            ;;
        *)
            error "unsupported OS: $OS (supported: macOS, Linux)"
            exit 1
            ;;
    esac
}

fetch_latest_version() {
    local version
    version=$(curl -fsSL --max-time 10 "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' | head -1 | sed -E 's/.*"v([^"]+)".*/\1/')
    if [ -z "$version" ]; then
        error "could not determine latest version from GitHub"
        error "check your internet connection or set GRAPHIQ_VERSION manually"
        exit 1
    fi
    echo "$version"
}

verify_checksum() {
    local archive="$1"
    local version="$2"
    local expected=""

    if command -v jq >/dev/null 2>&1; then
        expected=$(curl -fsSL --max-time 10 "https://api.github.com/repos/${REPO}/releases/tags/v${version}" \
            | jq -r --arg name "$(basename "$archive")" '.assets[] | select(.name == $name) | .digest // ""' \
            | sed -E 's/^sha256://')
    else
        expected=$(curl -fsSL --max-time 10 "https://api.github.com/repos/${REPO}/releases/tags/v${version}" \
            | awk -v name="$(basename "$archive")" '
                BEGIN { RS = "{"; in_asset = 0; asset_name = "" }
                /"name"\s*:\s*"/ { gsub(/.*"name"\s*:\s*"/, ""); gsub(/".*/, ""); asset_name = $0 }
                /"digest"\s*:\s*"/ && asset_name == name {
                    gsub(/.*"digest"\s*:\s*"/, ""); gsub(/".*/, "");
                    gsub(/^sha256:/, ""); print; found = 1; exit
                }
            ' 2>/dev/null)
    fi

    if [ -z "$expected" ]; then
        error "no checksum found in release metadata — cannot verify integrity"
        error "this may indicate a corrupted or incomplete release"
        exit 1
    fi

    local actual
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$archive" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$archive" | awk '{print $1}')
    else
        error "no sha256sum or shasum found — cannot verify integrity"
        error "install coreutils (Linux) or Xcode Command Line Tools (macOS)"
        exit 1
    fi

    if [ "$expected" != "$actual" ]; then
        error "SHA256 checksum mismatch"
        echo "    expected: ${expected}" >&2
        echo "    actual:   ${actual}" >&2
        exit 1
    fi
    info "checksum verified"
}

do_install() {
    echo ""
    echo "${BOLD}  GraphIQ Installer${RESET}"
    echo ""

    # Prerequisites
    need_cmd curl "https://curl.se/"
    need_cmd tar  "system package manager"

    if [ "$(uname -s)" = "Linux" ]; then
        if ! ldconfig -p 2>/dev/null | grep -q "libvulkan.so.1" && \
           ! [ -f /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ] && \
           ! [ -f /usr/lib/aarch64-linux-gnu/libvulkan.so.1 ]; then
            warn "Vulkan loader (libvulkan) not found — GPU acceleration requires it"
            if confirm "Install Vulkan loader now?"; then
                if command -v apt >/dev/null 2>&1; then
                    sudo apt install -y libvulkan1
                elif command -v dnf >/dev/null 2>&1; then
                    sudo dnf install -y vulkan-loader
                elif command -v pacman >/dev/null 2>&1; then
                    sudo pacman -S --noconfirm vulkan-driver
                else
                    echo "  Install manually: libvulkan1 (apt), vulkan-loader (dnf), vulkan-driver (pacman)"
                fi
            fi
        else
            info "Vulkan loader found"
        fi
    fi

    local platform
    platform="$(detect_platform)"
    local version="${GRAPHIQ_VERSION:-$(fetch_latest_version)}"

    local archive="graphiq-${platform}.tar.gz"
    local url="https://github.com/${REPO}/releases/download/v${version}/${archive}"

    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    echo "  version:  ${version}"
    echo "  platform: ${platform}"
    echo "  target:   ${INSTALL_DIR}"
    echo ""

    # Check for existing installation
    if command -v graphiq >/dev/null 2>&1; then
        local existing
        existing=$(graphiq --version 2>/dev/null || echo "unknown")
        echo "  existing: graphiq ${existing} at $(command -v graphiq)"
        echo ""
    fi

    echo "  downloading ${archive}..."
    if ! curl -fsSL --max-time 60 --retry 3 "$url" -o "${tmpdir}/${archive}"; then
        error "failed to download ${url}"
        echo "  The release may not include a binary for ${platform}." >&2
        echo "  Check available releases at:" >&2
        echo "    https://github.com/${REPO}/releases" >&2
        exit 1
    fi

    verify_checksum "${tmpdir}/${archive}" "$version"

    echo "  extracting..."
    tar xzf "${tmpdir}/${archive}" -C "$tmpdir"

    local need_sudo=""
    if [ ! -w "$INSTALL_DIR" ]; then
        need_sudo="sudo"
        if [ ! -d "$INSTALL_DIR" ]; then
            echo "  creating ${INSTALL_DIR}..."
            $need_sudo mkdir -p "$INSTALL_DIR"
        fi
    fi

    local installed=0
    for bin in graphiq graphiq-mcp graphiq-bench; do
        if [ -f "${tmpdir}/${bin}" ]; then
            $need_sudo cp "${tmpdir}/${bin}" "${INSTALL_DIR}/${bin}"
            $need_sudo chmod +x "${INSTALL_DIR}/${bin}"
            info "${bin} -> ${INSTALL_DIR}/${bin}"
            installed=$((installed + 1))
        else
            warn "${bin} not found in archive (may not be built for this platform)"
        fi
    done

    if [ "$installed" -eq 0 ]; then
        error "no binaries were installed"
        exit 1
    fi

    echo ""

    # Verify installation
    local fail=0
    if command -v graphiq >/dev/null 2>&1; then
        local ver
        ver=$(graphiq --version 2>/dev/null || echo "?")
        info "graphiq ${ver}"
    else
        error "graphiq not found on PATH after install"
        error "ensure ${INSTALL_DIR} is in your PATH"
        fail=1
    fi

    if command -v graphiq-mcp >/dev/null 2>&1; then
        info "graphiq-mcp available"
    else
        warn "graphiq-mcp not found on PATH (needed for MCP server mode)"
        fail=1
    fi

    if [ "$fail" -eq 1 ]; then
        echo ""
        echo "  Add to your shell profile:"
        echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
    fi

    echo ""
    echo "${BOLD}  Next steps:${RESET}"
    echo "    graphiq setup --project /path/to/project"
    echo "    graphiq index /path/to/project"
    echo "    graphiq search \"rate limit middleware\""
    echo "    graphiq impact --project /path/to/project"
    echo "    graphiq doctor"
    echo ""
}

do_uninstall() {
    echo "uninstalling graphiq..."
    for bin in graphiq graphiq-mcp graphiq-bench; do
        local target="${INSTALL_DIR}/${bin}"
        if [ -f "$target" ]; then
            local need_sudo=""
            if [ ! -w "$target" ]; then
                need_sudo="sudo"
            fi
            $need_sudo rm -f "$target"
            info "removed ${target}"
        fi
    done
    info "done"
}

case "$COMMAND" in
    install)
        do_install
        ;;
    uninstall)
        do_uninstall
        ;;
    *)
        echo "usage: $0 [install|uninstall]" >&2
        exit 1
        ;;
esac
