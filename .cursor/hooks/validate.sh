#!/usr/bin/env bash
# Logs every Cursor hook firing for manual validation.
# Log file: /tmp/cursor-hook-validation.log
#
# Before a test run:
#   .cursor/hooks/clear-log.sh
#
# During testing, watch live:
#   tail -f /tmp/cursor-hook-validation.log
set -euo pipefail

LOG=/tmp/cursor-hook-validation.log
EVENT="${1:-unknown}"
INPUT="$(cat)"

# One-line summary fields (best-effort parse).
summary="$(echo "$INPUT" | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    print("parse_error")
    sys.exit(0)
parts = []
for key in (
    "hook_event_name", "tool_name", "reason", "final_status", "status",
    "failure_type", "subagent_type", "composer_mode", "is_background_agent",
):
    if key in d and d[key] not in (None, ""):
        parts.append(f"{key}={d[key]!r}")
if d.get("workspace_roots"):
    parts.append(f"workspace={d['workspace_roots'][0]!r}")
if d.get("prompt"):
    p = d["prompt"]
    parts.append(f"prompt={p[:60]!r}{'…' if len(p) > 60 else ''}")
print(" ".join(parts) if parts else "(no summary fields)")
' 2>/dev/null || echo "parse_error")"

{
  echo "=== $(date -Iseconds) event=${EVENT} ==="
  echo "TMUX=${TMUX:-}"
  echo "TMUX_PANE=${TMUX_PANE:-}"
  echo "PWD=${PWD}"
  echo "summary: ${summary}"
  echo "payload_keys: $(echo "$INPUT" | python3 -c 'import json,sys; print(",".join(sorted(json.load(sys.stdin).keys())))' 2>/dev/null || echo parse_error)"
  echo "---"
} >>"$LOG"

exit 0
