#!/usr/bin/env bash

set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
  echo "ERROR: launchd status is only supported on macOS." >&2
  exit 1
fi

LABEL="${MAPFLOW_LAUNCHD_LABEL:-io.mapflow.server}"
LISTEN="${MAPFLOW_LISTEN:-127.0.0.1:3000}"
LAUNCH_AGENTS_DIR="${MAPFLOW_LAUNCH_AGENTS_DIR:-${HOME}/Library/LaunchAgents}"
TAIL_LINES=40

usage() {
  cat <<'EOF'
Usage:
  bash scripts/macos/launchd-status.sh [options]

Options:
  --label LABEL             launchd label (default: io.mapflow.server)
  --listen ADDR             listen addr for health check (default: 127.0.0.1:3000)
  --launch-agents-dir DIR   launch agents dir (default: ~/Library/LaunchAgents)
  --tail-lines N            tail last N log lines (default: 40)
  -h, --help                show help
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --label)
      LABEL="$2"
      shift 2
      ;;
    --listen)
      LISTEN="$2"
      shift 2
      ;;
    --launch-agents-dir)
      LAUNCH_AGENTS_DIR="$2"
      shift 2
      ;;
    --tail-lines)
      TAIL_LINES="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

PLIST_PATH="${LAUNCH_AGENTS_DIR}/${LABEL}.plist"
SERVICE_ID="gui/${UID}/${LABEL}"

if [ ! -f "$PLIST_PATH" ]; then
  echo "ERROR: plist not found: $PLIST_PATH" >&2
  exit 1
fi

stdout_log=""
stderr_log=""
if [ -x /usr/libexec/PlistBuddy ]; then
  stdout_log="$(/usr/libexec/PlistBuddy -c "Print :StandardOutPath" "$PLIST_PATH" 2>/dev/null || true)"
  stderr_log="$(/usr/libexec/PlistBuddy -c "Print :StandardErrorPath" "$PLIST_PATH" 2>/dev/null || true)"
fi

health_host="127.0.0.1"
health_port="3000"
if [[ "$LISTEN" == :* ]]; then
  health_port="${LISTEN#:}"
else
  health_host="${LISTEN%:*}"
  health_port="${LISTEN##*:}"
  if [ "$health_host" = "0.0.0.0" ] || [ "$health_host" = "" ]; then
    health_host="127.0.0.1"
  fi
fi
health_url="http://${health_host}:${health_port}/health"

echo "Service: $SERVICE_ID"
echo "Plist:   $PLIST_PATH"
echo
launchctl print "$SERVICE_ID" 2>/dev/null | sed -n '1,80p' || {
  echo "Service is not currently loaded."
}
echo
echo "Health check: $health_url"
if curl -fsS "$health_url" >/dev/null 2>&1; then
  echo "Health: OK"
else
  echo "Health: FAIL"
fi

if [ -n "$stdout_log" ]; then
  echo
  echo "stdout log: $stdout_log"
  if [ -f "$stdout_log" ]; then
    tail -n "$TAIL_LINES" "$stdout_log"
  else
    echo "(not created yet)"
  fi
fi

if [ -n "$stderr_log" ]; then
  echo
  echo "stderr log: $stderr_log"
  if [ -f "$stderr_log" ]; then
    tail -n "$TAIL_LINES" "$stderr_log"
  else
    echo "(not created yet)"
  fi
fi
