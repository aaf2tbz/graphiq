#!/usr/bin/env bash
# GraphIQ session:start hook
# Called when a Signet agent session starts in a project directory.
# Checks if the project has a fresh GraphIQ index and triggers background
# indexing if needed. Launches graphiq-mcp with --session-id if configured.

set -euo pipefail

PROJECT_ROOT="${SIGNET_PROJECT_ROOT:-.}"
SESSION_ID="${SIGNET_SESSION_ID:-}"
GRAPHIQ_DB="$PROJECT_ROOT/.graphiq/graphiq.db"

if [ ! -f "$GRAPHIQ_DB" ]; then
    echo "[graphiq] No index found for $PROJECT_ROOT, indexing in background..."
    graphiq index "$PROJECT_ROOT" --db "$GRAPHIQ_DB" &
    disown
fi
