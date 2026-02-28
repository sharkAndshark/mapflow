#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib/common.sh"

IMAGE=""
PORT="${SMOKE_PORT:-3000}"
FIXTURE_PATH="${SMOKE_FIXTURE:-frontend/tests/fixtures/sample.geojson}"
EXPECTED_B64_PATH="${SMOKE_EXPECTED_B64:-testdata/smoke/expected_sample_z0_x0_y0.mvt.base64}"
WORK_DIR="${SMOKE_WORK_DIR:-$(mktemp -d)}"
KEEP_DATA="${SMOKE_KEEP_DATA:-false}"

usage() {
  cat <<EOF
Usage: $(basename "$0") --image <name:tag> [options]

Options:
  --image <name:tag>    Docker image to test (required)
  --port <port>         Host port to use (default: 3000)
  --fixture <path>      Test file to upload (default: frontend/tests/fixtures/sample.geojson)
  --expected-b64 <path> Expected tile base64 file for verification
  --keep-data           Keep test data after completion

Environment:
  SMOKE_PORT            Default port
  SMOKE_FIXTURE         Default fixture path
  SMOKE_EXPECTED_B64    Default expected tile file
  SMOKE_WORK_DIR        Working directory (default: temp dir)
  SMOKE_KEEP_DATA       Keep data if "true"
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) IMAGE="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --fixture) FIXTURE_PATH="$2"; shift 2 ;;
    --expected-b64) EXPECTED_B64_PATH="$2"; shift 2 ;;
    --keep-data) KEEP_DATA="true"; shift ;;
    --help|-h) usage ;;
    *) smoke_fail "unknown option: $1" ;;
  esac
done

if [ -z "$IMAGE" ]; then
  smoke_fail "--image is required"
fi

if [ ! -f "$FIXTURE_PATH" ]; then
  smoke_fail "fixture not found: ${FIXTURE_PATH}"
fi

BASE_URL="http://127.0.0.1:${PORT}"
DATA_DIR="${WORK_DIR}/data"
UPLOADS_DIR="${WORK_DIR}/uploads"
COOKIE_JAR="${WORK_DIR}/cookies.txt"
TILE_OUT="${WORK_DIR}/tile.mvt"
PUBLIC_TILE_OUT="${WORK_DIR}/public_tile.mvt"

mkdir -p "$DATA_DIR" "$UPLOADS_DIR"

CONTAINER_ID=""

cleanup() {
  local exit_code=$?
  smoke_log "cleaning up..."
  if [ -n "$CONTAINER_ID" ]; then
    docker logs --tail 50 "$CONTAINER_ID" 2>&1 || true
    docker stop "$CONTAINER_ID" >/dev/null 2>&1 || true
  fi
  if [ "$KEEP_DATA" != "true" ] && [ -d "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR" || true
  fi
  exit "$exit_code"
}
trap cleanup EXIT

smoke_log "starting smoke test for image: ${IMAGE}"
smoke_log "work dir: ${WORK_DIR}"

CONTAINER_ID=$(docker run -d --rm \
  -p "${PORT}:3000" \
  -e LISTEN=:3000 \
  -e DB_PATH=/app/data/mapflow.duckdb \
  -e UPLOAD_DIR=/app/uploads \
  -v "${DATA_DIR}:/app/data" \
  -v "${UPLOADS_DIR}:/app/uploads" \
  "${IMAGE}")

smoke_log "container ID: ${CONTAINER_ID}"

wait_for_ready "$BASE_URL"

init_if_needed "$BASE_URL" "$COOKIE_JAR"
login "$BASE_URL" "$COOKIE_JAR"

FILE_ID=$(upload_file "$BASE_URL" "$COOKIE_JAR" "$FIXTURE_PATH")
smoke_log "uploaded file: ${FILE_ID}"

wait_for_status "$BASE_URL" "$COOKIE_JAR" "$FILE_ID" ready

get_tile "$BASE_URL" "$COOKIE_JAR" "$FILE_ID" 0 0 0 "$TILE_OUT"
verify_tile_content "$TILE_OUT" "$EXPECTED_B64_PATH"

SLUG=$(publish_file "$BASE_URL" "$COOKIE_JAR" "$FILE_ID")
smoke_log "published with slug: ${SLUG}"

get_public_tile "$BASE_URL" "$SLUG" 0 0 0 "$PUBLIC_TILE_OUT"

smoke_log "SUCCESS: all smoke tests passed"
