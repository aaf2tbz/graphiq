#!/usr/bin/env bash
# GraphIQ session:start hook
# Called when a Signet agent session starts in a project directory.
# Binds to the real provider workspace, then warms a session-scoped temp index.
# Uses a PID-based lock to prevent concurrent indexing.

set -euo pipefail

resolve_candidate_root() {
    local candidate="$1"

    if [ -z "$candidate" ]; then
        return 1
    fi

    if [ -f "$candidate" ] && [ "${candidate##*.}" = "db" ]; then
        candidate="$(dirname "$(dirname "$candidate")")"
    fi

    if [ ! -e "$candidate" ]; then
        return 1
    fi

    (
        cd "$candidate" 2>/dev/null || exit 1
        git rev-parse --show-toplevel 2>/dev/null || pwd
    )
}

is_hook_workspace() {
    local root="$1"
    local name
    name="$(basename "$root")"

    if [[ "$name" == graphiq-hook-* ]]; then
        return 0
    fi

    if [ -f "$root/src/lib.rs" ] && grep -q "hook_workspace_marker" "$root/src/lib.rs" 2>/dev/null; then
        return 0
    fi

    return 1
}

ENV_PROJECT_ROOT="${PROJECT_ROOT:-}"
PROJECT_ROOT=""
for value in \
    "${GRAPHIQ_PROJECT_ROOT:-}" \
    "${SIGNET_PROJECT_ROOT:-}" \
    "${CODEX_WORKSPACE_ROOT:-}" \
    "${CODEX_PROJECT_ROOT:-}" \
    "${CODEX_CWD:-}" \
    "${CLAUDE_PROJECT_DIR:-}" \
    "${WORKSPACE_ROOT:-}" \
    "$ENV_PROJECT_ROOT" \
    "${AGENT_WORKSPACE_ROOT:-}" \
    "${INIT_CWD:-}" \
    "${PWD:-.}"; do
    resolved="$(resolve_candidate_root "$value" || true)"
    if [ -n "$resolved" ] && ! is_hook_workspace "$resolved"; then
        PROJECT_ROOT="$resolved"
        break
    fi
done

if [ -z "$PROJECT_ROOT" ]; then
    PROJECT_ROOT="$(resolve_candidate_root "${PWD:-.}" || pwd)"
fi

SESSION_ID="${SIGNET_SESSION_ID:-${CODEX_SESSION_ID:-}}"
if [ -z "$SESSION_ID" ]; then
    if command -v shasum >/dev/null 2>&1; then
        SESSION_ID="$(printf "%s" "$PROJECT_ROOT" | shasum | awk '{print $1}')"
    else
        SESSION_ID="$(printf "%s" "$PROJECT_ROOT" | cksum | awk '{print $1}')"
    fi
fi

SESSION_DIR="${TMPDIR:-/tmp}/graphiq-session-${SESSION_ID}"
GRAPHIQ_DB="$SESSION_DIR/graphiq.db"
LOCKFILE="$SESSION_DIR/.index.lock"
BIND_FILE="$SESSION_DIR/project-root"
SIGNET_WORKSPACE="${SIGNET_WORKSPACE:-}"

if [ -n "$SIGNET_WORKSPACE" ]; then
    STATE_DIR="$SIGNET_WORKSPACE/.daemon/graphiq"
    STATE_FILE="$STATE_DIR/state.json"
    mkdir -p "$STATE_DIR"
    python3 - "$STATE_FILE" "$PROJECT_ROOT" "$SESSION_ID" "$GRAPHIQ_DB" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

state_path = Path(sys.argv[1])
project_root = str(Path(sys.argv[2]).resolve())
session_id = sys.argv[3] or None
db_path = str(Path(sys.argv[4]).resolve())

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
    "activeDbPath": db_path,
    "indexedProjects": indexed,
    "updatedAt": datetime.now(timezone.utc).isoformat(),
})
state_path.write_text(json.dumps(state, indent=2) + "\n")
PY
fi

if [ ! -f "$GRAPHIQ_DB" ]; then
    mkdir -p "$SESSION_DIR"
    printf "%s\n" "$PROJECT_ROOT" > "$BIND_FILE"

    if [ -f "$LOCKFILE" ]; then
        LOCK_PID=$(cat "$LOCKFILE" 2>/dev/null || echo "")
        if [ -n "$LOCK_PID" ] && kill -0 "$LOCK_PID" 2>/dev/null; then
            echo "[graphiq] Index already in progress (PID $LOCK_PID), skipping."
            exit 0
        fi
        rm -f "$LOCKFILE"
    fi

    echo "[graphiq] No session index found for $PROJECT_ROOT, indexing in background..."
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
