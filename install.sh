#!/usr/bin/env bash
# GraphIQ install/uninstall script
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/aaf2tbz/graphiq/main/install.sh | bash -s -- uninstall
set -euo pipefail

REPO="aaf2tbz/graphiq"
INSTALL_DIR="${GRAPHIQ_INSTALL_DIR:-/usr/local/bin}"
COMMAND="${1:-install}"

VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | head -1 | sed -E 's/.*"v([^"]+)".*/\1/')

if [ -z "$VERSION" ]; then
    echo "error: could not determine latest version"
    exit 1
fi

detect_platform() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    ARCH="$(uname -m)"

    case "$OS" in
        darwin)
            if [ "$ARCH" = "arm64" ]; then
                echo "aarch64-apple-darwin"
            else
                echo "x86_64-apple-darwin"
            fi
            ;;
        linux)
            echo "x86_64-unknown-linux-gnu"
            ;;
        *)
            echo "error: unsupported OS: $OS" >&2
            exit 1
            ;;
    esac
}

do_install() {
    PLATFORM="$(detect_platform)"
    ARCHIVE="graphiq-${PLATFORM}.tar.gz"
    URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ARCHIVE}"

    TMPDIR="$(mktemp -d)"
    trap 'rm -rf "$TMPDIR"' EXIT

    echo "graphiq v${VERSION} (${PLATFORM})"

    echo "  downloading ${ARCHIVE}..."
    curl -fsSL "$URL" -o "${TMPDIR}/${ARCHIVE}"

    echo "  verifying..."
    if command -v jq >/dev/null 2>&1; then
        EXPECTED="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/tags/v${VERSION}" | jq -r --arg name "$ARCHIVE" '.assets[] | select(.name == $name) | .digest // ""' | sed -E 's/^sha256://')"
    else
        EXPECTED="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/tags/v${VERSION}" | awk -v name="$ARCHIVE" '
            BEGIN { RS = "{"; in_asset = 0; asset_name = "" }
            /"name"\s*:\s*"/ { gsub(/.*"name"\s*:\s*"/, ""); gsub(/".*/, ""); asset_name = $0 }
            /"digest"\s*:\s*"/ && asset_name == name {
                gsub(/.*"digest"\s*:\s*"/, ""); gsub(/".*/, "");
                gsub(/^sha256:/, ""); print; found = 1; exit
            }
        ' 2>/dev/null)"
    fi

    if [ -z "$EXPECTED" ]; then
        echo "warning: no checksum found in release metadata, skipping verification"
    else
        ACTUAL="$(shasum -a 256 "${TMPDIR}/${ARCHIVE}" | awk '{print $1}')"
        if [ "$EXPECTED" != "$ACTUAL" ]; then
            echo "error: SHA256 mismatch"
            echo "  expected: ${EXPECTED}"
            echo "  actual:   ${ACTUAL}"
            exit 1
        fi
    fi

    echo "  extracting..."
    tar xzf "${TMPDIR}/${ARCHIVE}" -C "$TMPDIR"

    NEED_SUDO=""
    if [ ! -w "$INSTALL_DIR" ]; then
        NEED_SUDO="sudo"
        echo "  installing to ${INSTALL_DIR} (needs sudo)..."
    else
        echo "  installing to ${INSTALL_DIR}..."
    fi

    for bin in graphiq graphiq-mcp graphiq-bench; do
        if [ -f "${TMPDIR}/${bin}" ]; then
            $NEED_SUDO cp "${TMPDIR}/${bin}" "${INSTALL_DIR}/${bin}"
            $NEED_SUDO chmod +x "${INSTALL_DIR}/${bin}"
            echo "    ${bin} -> ${INSTALL_DIR}/${bin}"
        fi
    done

    echo ""
    echo "  installed graphiq v${VERSION}"
    echo "  try: graphiq index /path/to/project"
}

do_uninstall() {
    echo "uninstalling graphiq..."
    for bin in graphiq graphiq-mcp graphiq-bench; do
        TARGET="${INSTALL_DIR}/${bin}"
        if [ -f "$TARGET" ]; then
            NEED_SUDO=""
            if [ ! -w "$TARGET" ]; then
                NEED_SUDO="sudo"
            fi
            $NEED_SUDO rm -f "$TARGET"
            echo "  removed ${TARGET}"
        fi
    done
    echo "  done"
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
