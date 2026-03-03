#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib/common.sh"

IMAGE=""
PORT="${SMOKE_PORT:-3000}"
FIXTURE_PATH="${SMOKE_FIXTURE:-frontend/tests/fixtures/sample.geojson}"
MBTILES_FIXTURE_PATH="${SMOKE_MBTILES_FIXTURE:-testdata/monaco_roads.mbtiles}"
MBTILES_EXPECTED_FORMAT="${SMOKE_MBTILES_EXPECTED_FORMAT:-mvt}"
MBTILES_PNG_FIXTURE_PATH="${SMOKE_MBTILES_PNG_FIXTURE:-testdata/sample_png.mbtiles}"
MBTILES_PNG_EXPECTED_FORMAT="${SMOKE_MBTILES_PNG_EXPECTED_FORMAT:-png}"
OVERSIZE_FIXTURE_PATH="${SMOKE_OVERSIZE_FIXTURE:-frontend/tests/fixtures/roads.zip}"
OVERSIZE_LIMIT_MB="${SMOKE_OVERSIZE_LIMIT_MB:-1}"
CRS_UPDATE_INPUT="${SMOKE_CRS_UPDATE_INPUT:-urn:ogc:def:crs:EPSG::4490}"
CRS_UPDATE_EXPECTED="${SMOKE_CRS_UPDATE_EXPECTED:-EPSG:4490}"
CRS_UPDATE_EXPECTED_TYPE="${SMOKE_CRS_UPDATE_EXPECTED_TYPE:-standard}"
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
  --mbtiles-fixture <path> MBTiles test file (default: testdata/monaco_roads.mbtiles)
  --mbtiles-format <value> Expected MBTiles tileFormat (default: mvt)
  --mbtiles-png-fixture <path> PNG MBTiles test file (default: testdata/sample_png.mbtiles)
  --mbtiles-png-format <value> Expected PNG MBTiles tileFormat (default: png)
  --oversize-fixture <path>  Oversize test file (default: frontend/tests/fixtures/roads.zip)
  --oversize-limit-mb <n>    Temporary upload size limit for oversize check (default: 1)
  --crs-update-input <value> CRS value sent to PUT /api/files/:id/crs
  --crs-update-expected <value> Expected normalized CRS in response/meta
  --crs-update-type <value>  Expected crsType in response/meta
  --expected-b64 <path> Expected tile base64 file for verification
  --keep-data           Keep test data after completion

Environment:
  SMOKE_PORT            Default port
  SMOKE_FIXTURE         Default fixture path
  SMOKE_MBTILES_FIXTURE MBTiles test file path
  SMOKE_MBTILES_EXPECTED_FORMAT Expected MBTiles tileFormat
  SMOKE_MBTILES_PNG_FIXTURE PNG MBTiles test file path
  SMOKE_MBTILES_PNG_EXPECTED_FORMAT Expected PNG MBTiles tileFormat
  SMOKE_OVERSIZE_FIXTURE Oversize test file path
  SMOKE_OVERSIZE_LIMIT_MB Temporary upload size limit for oversize check
  SMOKE_CRS_UPDATE_INPUT CRS value sent to PUT /api/files/:id/crs
  SMOKE_CRS_UPDATE_EXPECTED Expected normalized CRS in response/meta
  SMOKE_CRS_UPDATE_EXPECTED_TYPE Expected crsType in response/meta
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
    --mbtiles-fixture) MBTILES_FIXTURE_PATH="$2"; shift 2 ;;
    --mbtiles-format) MBTILES_EXPECTED_FORMAT="$2"; shift 2 ;;
    --mbtiles-png-fixture) MBTILES_PNG_FIXTURE_PATH="$2"; shift 2 ;;
    --mbtiles-png-format) MBTILES_PNG_EXPECTED_FORMAT="$2"; shift 2 ;;
    --oversize-fixture) OVERSIZE_FIXTURE_PATH="$2"; shift 2 ;;
    --oversize-limit-mb) OVERSIZE_LIMIT_MB="$2"; shift 2 ;;
    --crs-update-input) CRS_UPDATE_INPUT="$2"; shift 2 ;;
    --crs-update-expected) CRS_UPDATE_EXPECTED="$2"; shift 2 ;;
    --crs-update-type) CRS_UPDATE_EXPECTED_TYPE="$2"; shift 2 ;;
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

if [ ! -f "$MBTILES_FIXTURE_PATH" ]; then
  smoke_fail "mbtiles fixture not found: ${MBTILES_FIXTURE_PATH}"
fi

if [ ! -f "$MBTILES_PNG_FIXTURE_PATH" ]; then
  smoke_fail "mbtiles png fixture not found: ${MBTILES_PNG_FIXTURE_PATH}"
fi

if [ ! -f "$OVERSIZE_FIXTURE_PATH" ]; then
  smoke_fail "oversize fixture not found: ${OVERSIZE_FIXTURE_PATH}"
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

INVALID_FIXTURE_PATH="${WORK_DIR}/invalid-upload.txt"
echo "not-a-geospatial-format" > "$INVALID_FIXTURE_PATH"
verify_invalid_upload_rejected "$BASE_URL" "$COOKIE_JAR" "$INVALID_FIXTURE_PATH"

FILE_ID=$(upload_file "$BASE_URL" "$COOKIE_JAR" "$FIXTURE_PATH")
smoke_log "uploaded file: ${FILE_ID}"

wait_for_status "$BASE_URL" "$COOKIE_JAR" "$FILE_ID" ready

verify_schema_endpoint "$BASE_URL" "$COOKIE_JAR" "$FILE_ID"
verify_feature_properties_endpoint "$BASE_URL" "$COOKIE_JAR" "$FILE_ID" 1
verify_crs_update "$BASE_URL" "$COOKIE_JAR" "$FILE_ID" "$CRS_UPDATE_INPUT" "$CRS_UPDATE_EXPECTED" "$CRS_UPDATE_EXPECTED_TYPE"

get_tile "$BASE_URL" "$COOKIE_JAR" "$FILE_ID" 0 0 0 "$TILE_OUT"
verify_tile_content "$TILE_OUT" "$EXPECTED_B64_PATH"

SLUG=$(publish_file "$BASE_URL" "$COOKIE_JAR" "$FILE_ID")
smoke_log "published with slug: ${SLUG}"

get_public_tile "$BASE_URL" "$SLUG" 0 0 0 "$PUBLIC_TILE_OUT"

MBTILES_FILE_ID=$(upload_file "$BASE_URL" "$COOKIE_JAR" "$MBTILES_FIXTURE_PATH")
smoke_log "uploaded MBTiles(MVT) file: ${MBTILES_FILE_ID}"

wait_for_status "$BASE_URL" "$COOKIE_JAR" "$MBTILES_FILE_ID" ready
verify_mbtiles_preview_meta "$BASE_URL" "$COOKIE_JAR" "$MBTILES_FILE_ID" "$MBTILES_EXPECTED_FORMAT"
verify_mbtiles_schema_endpoint "$BASE_URL" "$COOKIE_JAR" "$MBTILES_FILE_ID"
verify_mbtiles_feature_properties_rejected "$BASE_URL" "$COOKIE_JAR" "$MBTILES_FILE_ID"

MBTILES_SLUG=$(publish_file "$BASE_URL" "$COOKIE_JAR" "$MBTILES_FILE_ID")
smoke_log "published MBTiles(MVT) with slug: ${MBTILES_SLUG}"
verify_public_tile_meta_format "$BASE_URL" "$MBTILES_SLUG" "$MBTILES_EXPECTED_FORMAT"

MBTILES_PNG_FILE_ID=$(upload_file "$BASE_URL" "$COOKIE_JAR" "$MBTILES_PNG_FIXTURE_PATH")
smoke_log "uploaded MBTiles(PNG) file: ${MBTILES_PNG_FILE_ID}"

wait_for_status "$BASE_URL" "$COOKIE_JAR" "$MBTILES_PNG_FILE_ID" ready
verify_mbtiles_preview_meta "$BASE_URL" "$COOKIE_JAR" "$MBTILES_PNG_FILE_ID" "$MBTILES_PNG_EXPECTED_FORMAT"
verify_mbtiles_schema_empty_endpoint "$BASE_URL" "$COOKIE_JAR" "$MBTILES_PNG_FILE_ID"
verify_mbtiles_feature_properties_rejected "$BASE_URL" "$COOKIE_JAR" "$MBTILES_PNG_FILE_ID"
verify_private_tile_content_type "$BASE_URL" "$COOKIE_JAR" "$MBTILES_PNG_FILE_ID" "image/png"

MBTILES_PNG_SLUG=$(publish_file "$BASE_URL" "$COOKIE_JAR" "$MBTILES_PNG_FILE_ID")
smoke_log "published MBTiles(PNG) with slug: ${MBTILES_PNG_SLUG}"
verify_public_tile_meta_format "$BASE_URL" "$MBTILES_PNG_SLUG" "$MBTILES_PNG_EXPECTED_FORMAT"
verify_public_tile_content_type "$BASE_URL" "$MBTILES_PNG_SLUG" "image/png"

oversize_bytes=$(wc -c < "$OVERSIZE_FIXTURE_PATH" | tr -d ' ')
oversize_limit_bytes=$((OVERSIZE_LIMIT_MB * 1024 * 1024))
if [ "$oversize_bytes" -le "$oversize_limit_bytes" ]; then
  smoke_fail "oversize fixture is not larger than limit (${oversize_bytes} <= ${oversize_limit_bytes})"
fi

set_upload_max_size_mb "$BASE_URL" "$COOKIE_JAR" "$OVERSIZE_LIMIT_MB"
verify_oversize_upload_rejected "$BASE_URL" "$COOKIE_JAR" "$OVERSIZE_FIXTURE_PATH"

smoke_log "SUCCESS: all smoke tests passed"
