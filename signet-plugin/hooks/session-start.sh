#!/usr/bin/env bash
# GraphIQ session:start hook
# Called when a Signet agent session starts in a project directory.
# Checks if the project has a fresh GraphIQ index and triggers background
# indexing if needed. Uses a PID-based lock to prevent concurrent indexing.

set -euo pipefail

PROJECT_ROOT="${SIGNET_PROJECT_ROOT:-.}"
SESSION_ID="${SIGNET_SESSION_ID:-}"
GRAPHIQ_DB="$PROJECT_ROOT/.graphiq/graphiq.db"
LOCKFILE="$PROJECT_ROOT/.graphiq/.index.lock"

if [ ! -f "$GRAPHIQ_DB" ]; then
    mkdir -p "$(dirname "$GRAPHIQ_DB")"

    if [ -f "$LOCKFILE" ]; then
        LOCK_PID=$(cat "$LOCKFILE" 2>/dev/null || echo "")
        if [ -n "$LOCK_PID" ] && kill -0 "$LOCK_PID" 2>/dev/null; then
            echo "[graphiq] Index already in progress (PID $LOCK_PID), skipping."
            exit 0
        fi
        rm -f "$LOCKFILE"
    fi

    echo "[graphiq] No index found for $PROJECT_ROOT, indexing in background..."
    (
        echo $$ > "$LOCKFILE"
        trap 'rm -f "$LOCKFILE"' EXIT
        graphiq index "$PROJECT_ROOT" --db "$GRAPHIQ_DB"
    ) &
    disown
fi
