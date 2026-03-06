#!/usr/bin/env bash

set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
  echo "ERROR: launchd uninstall is only supported on macOS." >&2
  exit 1
fi

if ! command -v launchctl >/dev/null 2>&1; then
  echo "ERROR: launchctl not found." >&2
  exit 1
fi

LABEL="${MAPFLOW_LAUNCHD_LABEL:-io.mapflow.server}"
LAUNCH_AGENTS_DIR="${MAPFLOW_LAUNCH_AGENTS_DIR:-${HOME}/Library/LaunchAgents}"
KEEP_PLIST="false"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/macos/launchd-uninstall.sh [options]

Options:
  --label LABEL             launchd label (default: io.mapflow.server)
  --launch-agents-dir DIR   launch agents dir (default: ~/Library/LaunchAgents)
  --keep-plist              only unload service, keep plist file
  -h, --help                show help
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --label)
      LABEL="$2"
      shift 2
      ;;
    --launch-agents-dir)
      LAUNCH_AGENTS_DIR="$2"
      shift 2
      ;;
    --keep-plist)
      KEEP_PLIST="true"
      shift
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

launchctl bootout "$SERVICE_ID" >/dev/null 2>&1 || true
launchctl disable "$SERVICE_ID" >/dev/null 2>&1 || true

if [ "$KEEP_PLIST" != "true" ]; then
  rm -f "$PLIST_PATH"
  echo "Unloaded and removed: $PLIST_PATH"
else
  echo "Unloaded service and kept plist: $PLIST_PATH"
fi

echo "Service ID: $SERVICE_ID"
