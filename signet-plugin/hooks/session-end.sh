#!/usr/bin/env bash
# GraphIQ session:end hook
# Called when a Signet agent session ends.
# Cleans up ephemeral session-scoped databases.

set -euo pipefail

SESSION_ID="${SIGNET_SESSION_ID:-${CODEX_SESSION_ID:-}}"

if [ -z "$SESSION_ID" ]; then
    PROJECT_ROOT="${GRAPHIQ_PROJECT_ROOT:-${SIGNET_PROJECT_ROOT:-${CODEX_WORKSPACE_ROOT:-${CODEX_PROJECT_ROOT:-${CODEX_CWD:-${CLAUDE_PROJECT_DIR:-${WORKSPACE_ROOT:-${PROJECT_ROOT:-${AGENT_WORKSPACE_ROOT:-${INIT_CWD:-${PWD:-}}}}}}}}}}}"
    if [ -n "$PROJECT_ROOT" ]; then
        if [ -e "$PROJECT_ROOT" ]; then
            PROJECT_ROOT="$(
                cd "$PROJECT_ROOT" 2>/dev/null && git rev-parse --show-toplevel 2>/dev/null || printf "%s" "$PROJECT_ROOT"
            )"
        fi
        if command -v shasum >/dev/null 2>&1; then
            SESSION_ID="$(printf "%s" "$PROJECT_ROOT" | shasum | awk '{print $1}')"
        else
            SESSION_ID="$(printf "%s" "$PROJECT_ROOT" | cksum | awk '{print $1}')"
        fi
    fi
fi

if [ -n "$SESSION_ID" ]; then
    SESSION_DIR="${TMPDIR:-/tmp}/graphiq-session-${SESSION_ID}"
    if [ -d "$SESSION_DIR" ]; then
        rm -rf "$SESSION_DIR"
        echo "[graphiq] Cleaned up session DB: $SESSION_DIR"
    fi
fi
