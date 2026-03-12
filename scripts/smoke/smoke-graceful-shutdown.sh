#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib/common.sh"

BINARY_PATH=""
PORT="${SMOKE_GRACEFUL_PORT:-3011}"
FIXTURE_PATH="${SMOKE_FIXTURE:-frontend/tests/fixtures/sample.geojson}"
WORK_DIR="${SMOKE_WORK_DIR:-$(mktemp -d)}"
KEEP_DATA="${SMOKE_KEEP_DATA:-false}"

DB_PATH_FILE="${WORK_DIR}/data/mapflow.duckdb"
UPLOADS_DIR="${WORK_DIR}/uploads"
COOKIE_JAR="${WORK_DIR}/cookies.txt"
RESTART_COOKIE_JAR="${WORK_DIR}/cookies-restart.txt"
BASE_URL="http://127.0.0.1:${PORT}"
FIRST_LOG="${WORK_DIR}/server-first.log"
SECOND_LOG="${WORK_DIR}/server-second.log"
PID=""

usage() {
  cat <<EOF
Usage: $(basename "$0") --binary <path> [options]

Options:
  --binary <path>       Path to mapflow binary (required)
  --port <port>         Port to use (default: 3011)
  --fixture <path>      Test file to upload (default: frontend/tests/fixtures/sample.geojson)
  --keep-data           Keep test data after completion

Environment:
  SMOKE_GRACEFUL_PORT   Default graceful-shutdown test port
  SMOKE_FIXTURE         Default fixture path
  SMOKE_WORK_DIR        Working directory (default: temp dir)
  SMOKE_KEEP_DATA       Keep data if "true"
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BINARY_PATH="$2"; shift 2 ;;
    --port) PORT="$2"; BASE_URL="http://127.0.0.1:${PORT}"; shift 2 ;;
    --fixture) FIXTURE_PATH="$2"; shift 2 ;;
    --keep-data) KEEP_DATA="true"; shift ;;
    --help|-h) usage ;;
    *) smoke_fail "unknown option: $1" ;;
  esac
done

if is_windows_shell; then
  smoke_fail "graceful-shutdown smoke is Unix-only (requires SIGHUP)"
fi

if [ -z "$BINARY_PATH" ]; then
  smoke_fail "--binary is required"
fi

if [ ! -f "$BINARY_PATH" ]; then
  smoke_fail "binary not found: ${BINARY_PATH}"
fi

if [ ! -f "$FIXTURE_PATH" ]; then
  smoke_fail "fixture not found: ${FIXTURE_PATH}"
fi

mkdir -p "$(dirname "$DB_PATH_FILE")" "$UPLOADS_DIR"

start_server() {
  local log_path="$1"
  DB_PATH="$DB_PATH_FILE" \
  UPLOAD_DIR="$UPLOADS_DIR" \
  LISTEN="127.0.0.1:${PORT}" \
    "$BINARY_PATH" >"$log_path" 2>&1 &
  PID=$!
  smoke_log "server PID: ${PID} (log: ${log_path})"
}

stop_server_force() {
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    force_kill_pid "$PID"
    wait "$PID" 2>/dev/null || true
  fi
  PID=""
}

wait_for_exit() {
  local pid="$1"
  local max_tries="${2:-80}"
  local delay="${3:-0.25}"
  for _ in $(seq 1 "$max_tries"); do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 0
    fi
    sleep "$delay"
  done
  return 1
}

cleanup() {
  local exit_code=$?
  smoke_log "cleaning up..."
  stop_server_force

  if [ "$exit_code" -ne 0 ]; then
    if [ -f "$FIRST_LOG" ]; then
      echo "[smoke] --- first start log tail ---" >&2
      tail -n 120 "$FIRST_LOG" >&2 || true
    fi
    if [ -f "$SECOND_LOG" ]; then
      echo "[smoke] --- second start log tail ---" >&2
      tail -n 120 "$SECOND_LOG" >&2 || true
    fi
  fi

  if [ "$KEEP_DATA" != "true" ] && [ -d "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR" || true
  fi
  exit "$exit_code"
}
trap cleanup EXIT

smoke_log "starting graceful-shutdown smoke for binary: ${BINARY_PATH}"
smoke_log "work dir: ${WORK_DIR}"

start_server "$FIRST_LOG"
wait_for_ready "$BASE_URL"
init_if_needed "$BASE_URL" "$COOKIE_JAR"
login "$BASE_URL" "$COOKIE_JAR"

FILE_ID=$(upload_file "$BASE_URL" "$COOKIE_JAR" "$FIXTURE_PATH")
smoke_log "uploaded file before SIGHUP: ${FILE_ID}"
wait_for_status "$BASE_URL" "$COOKIE_JAR" "$FILE_ID" ready

smoke_log "sending SIGHUP to trigger graceful shutdown"
kill -HUP "$PID"
if ! wait_for_exit "$PID"; then
  smoke_fail "server did not exit after SIGHUP"
fi
wait "$PID" 2>/dev/null || true
PID=""

if ! grep -q "Database checkpoint completed" "$FIRST_LOG"; then
  smoke_fail "missing checkpoint completion log after SIGHUP"
fi

start_server "$SECOND_LOG"
wait_for_ready "$BASE_URL"
rm -f "$RESTART_COOKIE_JAR"
init_if_needed "$BASE_URL" "$RESTART_COOKIE_JAR"
login "$BASE_URL" "$RESTART_COOKIE_JAR"

FILES_JSON="${WORK_DIR}/files-after-restart.json"
curl_with_retry -fsS -b "$RESTART_COOKIE_JAR" "${BASE_URL}/api/files" > "$FILES_JSON"
pre_hup_file_visibility=$(python3 - "$FILES_JSON" "$FILE_ID" <<'PY'
import json
import sys

path, expected_id = sys.argv[1], sys.argv[2]
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

if not isinstance(data, list):
    raise SystemExit("expected /api/files to return a JSON array")

found = False
for item in data:
    if item.get("id") == expected_id:
        found = True
        break

print("present" if found else "missing")
PY
)
smoke_log "pre-SIGHUP file visibility after restart: ${pre_hup_file_visibility}"
if [ "$pre_hup_file_visibility" != "present" ]; then
  smoke_fail "uploaded file is missing after graceful shutdown restart"
fi

backup_count=$(find "$(dirname "$DB_PATH_FILE")" -maxdepth 1 -name "$(basename "$DB_PATH_FILE").wal.bak.*" | wc -l | tr -d ' ')
smoke_log "WAL backup count after graceful restart: ${backup_count}"
if [ "$backup_count" != "0" ]; then
  smoke_fail "unexpected WAL backup artifacts after graceful shutdown"
fi

POST_RESTART_FILE_ID=$(upload_file "$BASE_URL" "$RESTART_COOKIE_JAR" "$FIXTURE_PATH")
smoke_log "uploaded file after graceful restart: ${POST_RESTART_FILE_ID}"
wait_for_status "$BASE_URL" "$RESTART_COOKIE_JAR" "$POST_RESTART_FILE_ID" ready

smoke_log "SUCCESS: graceful-shutdown smoke test passed"
