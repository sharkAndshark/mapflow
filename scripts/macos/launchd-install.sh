#!/usr/bin/env bash

set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
  echo "ERROR: launchd install is only supported on macOS." >&2
  exit 1
fi

if ! command -v launchctl >/dev/null 2>&1; then
  echo "ERROR: launchctl not found." >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_LABEL="${MAPFLOW_LAUNCHD_LABEL:-io.mapflow.server}"
DEFAULT_LISTEN="${MAPFLOW_LISTEN:-127.0.0.1:3000}"
DEFAULT_HOME="${MAPFLOW_HOME:-${HOME}/.mapflow}"
DEFAULT_DB_PATH="${MAPFLOW_DB_PATH:-${DEFAULT_HOME}/data/mapflow.duckdb}"
DEFAULT_UPLOAD_DIR="${MAPFLOW_UPLOAD_DIR:-${DEFAULT_HOME}/uploads}"
DEFAULT_STDOUT_LOG="${MAPFLOW_STDOUT_LOG:-${DEFAULT_HOME}/logs/server.stdout.log}"
DEFAULT_STDERR_LOG="${MAPFLOW_STDERR_LOG:-${DEFAULT_HOME}/logs/server.stderr.log}"
DEFAULT_LAUNCH_AGENTS_DIR="${MAPFLOW_LAUNCH_AGENTS_DIR:-${HOME}/Library/LaunchAgents}"

LABEL="$DEFAULT_LABEL"
LISTEN="$DEFAULT_LISTEN"
MAPFLOW_HOME="$DEFAULT_HOME"
DB_PATH="$DEFAULT_DB_PATH"
UPLOAD_DIR="$DEFAULT_UPLOAD_DIR"
STDOUT_LOG="$DEFAULT_STDOUT_LOG"
STDERR_LOG="$DEFAULT_STDERR_LOG"
LAUNCH_AGENTS_DIR="$DEFAULT_LAUNCH_AGENTS_DIR"
WEB_DIST="${MAPFLOW_WEB_DIST:-}"
BINARY_PATH="${MAPFLOW_BINARY:-}"
FORCE="false"
DRY_RUN="false"
EXTRA_ENV=()
HOME_SET="false"
DB_PATH_SET="false"
UPLOAD_DIR_SET="false"
STDOUT_LOG_SET="false"
STDERR_LOG_SET="false"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/macos/launchd-install.sh --binary /abs/path/to/mapflow [options]

Options:
  --binary PATH             mapflow binary path (required unless auto-detected)
  --label LABEL             launchd label (default: io.mapflow.server)
  --listen ADDR             listen address for backend (default: 127.0.0.1:3000)
  --home DIR                runtime home directory (default: ~/.mapflow)
  --db-path PATH            DB_PATH env (default: ~/.mapflow/data/mapflow.duckdb)
  --upload-dir PATH         UPLOAD_DIR env (default: ~/.mapflow/uploads)
  --stdout-log PATH         StandardOutPath in plist (default: ~/.mapflow/logs/server.stdout.log)
  --stderr-log PATH         StandardErrorPath in plist (default: ~/.mapflow/logs/server.stderr.log)
  --web-dist PATH           WEB_DIST env (optional)
  --launch-agents-dir DIR   launch agents dir (default: ~/Library/LaunchAgents)
  --env KEY=VALUE           additional env var (repeatable)
  --force                   replace existing plist/loaded service
  --dry-run                 write plist but do not bootstrap service
  -h, --help                show help
EOF
}

xml_escape() {
  local s="$1"
  s="${s//&/&amp;}"
  s="${s//</&lt;}"
  s="${s//>/&gt;}"
  s="${s//\"/&quot;}"
  s="${s//\'/&apos;}"
  printf '%s' "$s"
}

abs_path() {
  local target="$1"
  if [ -d "$target" ]; then
    (cd "$target" && pwd -P)
  else
    (cd "$(dirname "$target")" && printf '%s/%s\n' "$(pwd -P)" "$(basename "$target")")
  fi
}

guess_binary() {
  local candidates=(
    "${PWD}/mapflow"
    "${SCRIPT_DIR}/../../mapflow"
    "${PWD}/target/release/backend"
  )
  local candidate
  for candidate in "${candidates[@]}"; do
    if [ -x "$candidate" ]; then
      abs_path "$candidate"
      return 0
    fi
  done
  return 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --binary)
      BINARY_PATH="$2"
      shift 2
      ;;
    --label)
      LABEL="$2"
      shift 2
      ;;
    --listen)
      LISTEN="$2"
      shift 2
      ;;
    --home)
      MAPFLOW_HOME="$2"
      HOME_SET="true"
      shift 2
      ;;
    --db-path)
      DB_PATH="$2"
      DB_PATH_SET="true"
      shift 2
      ;;
    --upload-dir)
      UPLOAD_DIR="$2"
      UPLOAD_DIR_SET="true"
      shift 2
      ;;
    --stdout-log)
      STDOUT_LOG="$2"
      STDOUT_LOG_SET="true"
      shift 2
      ;;
    --stderr-log)
      STDERR_LOG="$2"
      STDERR_LOG_SET="true"
      shift 2
      ;;
    --web-dist)
      WEB_DIST="$2"
      shift 2
      ;;
    --launch-agents-dir)
      LAUNCH_AGENTS_DIR="$2"
      shift 2
      ;;
    --env)
      EXTRA_ENV+=("$2")
      shift 2
      ;;
    --force)
      FORCE="true"
      shift
      ;;
    --dry-run)
      DRY_RUN="true"
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

if [ -z "$BINARY_PATH" ]; then
  if ! BINARY_PATH="$(guess_binary)"; then
    echo "ERROR: --binary is required (or place executable at ./mapflow)." >&2
    exit 1
  fi
fi

BINARY_PATH="$(abs_path "$BINARY_PATH")"

if [ ! -x "$BINARY_PATH" ]; then
  echo "ERROR: binary is not executable: $BINARY_PATH" >&2
  exit 1
fi

if [ "${#EXTRA_ENV[@]}" -gt 0 ]; then
  for kv in "${EXTRA_ENV[@]}"; do
    if [[ "$kv" != *=* ]]; then
      echo "ERROR: --env must be KEY=VALUE: $kv" >&2
      exit 1
    fi
  done
fi

mkdir -p "$LAUNCH_AGENTS_DIR" "$MAPFLOW_HOME"
LAUNCH_AGENTS_DIR="$(abs_path "$LAUNCH_AGENTS_DIR")"
MAPFLOW_HOME="$(abs_path "$MAPFLOW_HOME")"

if [ "$HOME_SET" = "true" ]; then
  if [ "$DB_PATH_SET" != "true" ]; then
    DB_PATH="${MAPFLOW_HOME}/data/mapflow.duckdb"
  fi
  if [ "$UPLOAD_DIR_SET" != "true" ]; then
    UPLOAD_DIR="${MAPFLOW_HOME}/uploads"
  fi
  if [ "$STDOUT_LOG_SET" != "true" ]; then
    STDOUT_LOG="${MAPFLOW_HOME}/logs/server.stdout.log"
  fi
  if [ "$STDERR_LOG_SET" != "true" ]; then
    STDERR_LOG="${MAPFLOW_HOME}/logs/server.stderr.log"
  fi
fi

mkdir -p "$MAPFLOW_HOME" "$(dirname "$DB_PATH")" "$UPLOAD_DIR" "$(dirname "$STDOUT_LOG")" "$(dirname "$STDERR_LOG")"
touch "$STDOUT_LOG" "$STDERR_LOG"

PLIST_PATH="${LAUNCH_AGENTS_DIR}/${LABEL}.plist"
SERVICE_ID="gui/${UID}/${LABEL}"
WORKING_DIR="$(dirname "$BINARY_PATH")"

if [ -f "$PLIST_PATH" ] && [ "$FORCE" != "true" ]; then
  echo "ERROR: plist already exists: $PLIST_PATH (use --force to replace)" >&2
  exit 1
fi

tmp_plist="$(mktemp)"
cleanup_tmp() {
  rm -f "$tmp_plist"
}
trap cleanup_tmp EXIT

{
  cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$(xml_escape "$LABEL")</string>
  <key>ProgramArguments</key>
  <array>
    <string>$(xml_escape "$BINARY_PATH")</string>
    <string>--listen</string>
    <string>$(xml_escape "$LISTEN")</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>DB_PATH</key>
    <string>$(xml_escape "$DB_PATH")</string>
    <key>UPLOAD_DIR</key>
    <string>$(xml_escape "$UPLOAD_DIR")</string>
EOF
  if [ -n "$WEB_DIST" ]; then
    cat <<EOF
    <key>WEB_DIST</key>
    <string>$(xml_escape "$WEB_DIST")</string>
EOF
  fi
  if [ "${#EXTRA_ENV[@]}" -gt 0 ]; then
    for kv in "${EXTRA_ENV[@]}"; do
      key="${kv%%=*}"
      value="${kv#*=}"
      cat <<EOF
    <key>$(xml_escape "$key")</key>
    <string>$(xml_escape "$value")</string>
EOF
    done
  fi
  cat <<EOF
  </dict>
  <key>WorkingDirectory</key>
  <string>$(xml_escape "$WORKING_DIR")</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>$(xml_escape "$STDOUT_LOG")</string>
  <key>StandardErrorPath</key>
  <string>$(xml_escape "$STDERR_LOG")</string>
</dict>
</plist>
EOF
} > "$tmp_plist"

cp "$tmp_plist" "$PLIST_PATH"
chmod 0644 "$PLIST_PATH"

echo "Installed plist: $PLIST_PATH"

if [ "$DRY_RUN" = "true" ]; then
  echo "Dry-run enabled: skip launchctl bootstrap."
  echo "Next:"
  echo "  launchctl bootstrap gui/${UID} \"$PLIST_PATH\""
  echo "  launchctl kickstart -k \"$SERVICE_ID\""
  exit 0
fi

launchctl bootout "$SERVICE_ID" >/dev/null 2>&1 || true
launchctl bootstrap "gui/${UID}" "$PLIST_PATH"
launchctl enable "$SERVICE_ID" >/dev/null 2>&1 || true
launchctl kickstart -k "$SERVICE_ID"

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

echo
echo "Service installed and started."
echo "Self-check commands:"
echo "  bash scripts/macos/launchd-status.sh --label \"$LABEL\" --listen \"$LISTEN\""
echo "  curl -fsS \"$health_url\""
echo "Logs:"
echo "  tail -f \"$STDOUT_LOG\""
echo "  tail -f \"$STDERR_LOG\""
