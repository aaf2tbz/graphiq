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
SIGNET_WORKSPACE="${SIGNET_WORKSPACE:-}"

if [ -n "$SIGNET_WORKSPACE" ]; then
    STATE_DIR="$SIGNET_WORKSPACE/.daemon/graphiq"
    STATE_FILE="$STATE_DIR/state.json"
    mkdir -p "$STATE_DIR"
    python3 - "$STATE_FILE" "$PROJECT_ROOT" "$SESSION_ID" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

state_path = Path(sys.argv[1])
project_root = str(Path(sys.argv[2]).resolve())
session_id = sys.argv[3] or None

try:
    state = json.loads(state_path.read_text())
    if not isinstance(state, dict):
        state = {}
except Exception:
    state = {}

indexed = state.get("indexedProjects")
if not isinstance(indexed, list):
    indexed = []
if project_root not in indexed:
    indexed.append(project_root)

state.update({
    "pluginId": "graphiq",
    "enabled": True,
    "activeProject": project_root,
    "activeSessionId": session_id,
    "indexedProjects": indexed,
    "updatedAt": datetime.now(timezone.utc).isoformat(),
})
state_path.write_text(json.dumps(state, indent=2) + "\n")
PY
fi

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
        GRAPHIQ_INDEX_MODE=background \
        GRAPHIQ_SOURCE_TERM_LIMIT="${GRAPHIQ_SOURCE_TERM_LIMIT:-1200}" \
        RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-2}" \
        graphiq index "$PROJECT_ROOT" --db "$GRAPHIQ_DB"
    ) &
    disown
fi
