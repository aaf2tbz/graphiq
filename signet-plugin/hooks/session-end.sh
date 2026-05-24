#!/usr/bin/env bash
# GraphIQ session:end hook
# Called when a Signet agent session ends.
# Cleans up ephemeral session-scoped databases.

set -euo pipefail

SESSION_ID="${SIGNET_SESSION_ID:-}"

if [ -n "$SESSION_ID" ]; then
    SESSION_DIR="${TMPDIR:-/tmp}/graphiq-session-${SESSION_ID}"
    if [ -d "$SESSION_DIR" ]; then
        rm -rf "$SESSION_DIR"
        echo "[graphiq] Cleaned up session DB: $SESSION_DIR"
    fi
fi
