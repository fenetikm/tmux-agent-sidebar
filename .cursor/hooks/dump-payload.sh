#!/usr/bin/env bash
# Dumps full JSON payloads for adapter design. Separate from validate.sh so
# the main log stays readable during manual testing.
# Log file: /tmp/cursor-hook-payloads.log
set -euo pipefail

LOG=/tmp/cursor-hook-payloads.log
EVENT="${1:-unknown}"
INPUT="$(cat)"

{
  echo "=== $(date -Iseconds) ${EVENT} TMUX_PANE=${TMUX_PANE:-} ==="
  echo "$INPUT" | python3 -m json.tool 2>/dev/null || echo "$INPUT"
  echo
} >>"$LOG"

exit 0
