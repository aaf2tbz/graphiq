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
    # DORMANCY: terminate any background indexing still running for this
    # session so graphiq stops doing work the moment the session/harness
    # closes (the plan: "if the user is not inside of an active harness or
    # project, graphiq should remain dormant"). The session-start hook writes
    # the indexer PID to .index.lock; if it's still alive, stop it cleanly.
    LOCKFILE="$SESSION_DIR/.index.lock"
    if [ -f "$LOCKFILE" ]; then
        LOCK_PID="$(cat "$LOCKFILE" 2>/dev/null || echo "")"
        if [ -n "$LOCK_PID" ] && kill -0 "$LOCK_PID" 2>/dev/null; then
            kill "$LOCK_PID" 2>/dev/null || true
            echo "[graphiq] Stopped background indexer (PID $LOCK_PID) for ended session."
        fi
        rm -f "$LOCKFILE"
    fi
    if [ -d "$SESSION_DIR" ]; then
        rm -rf "$SESSION_DIR"
        echo "[graphiq] Cleaned up session DB: $SESSION_DIR"
    fi
fi
