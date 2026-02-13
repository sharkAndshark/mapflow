#!/usr/bin/env bash

set -euo pipefail

image="${SMOKE_IMAGE:-mapflow-smoke:ci}"
port="${SMOKE_PORT:-39000}"
runner_tmp="${RUNNER_TEMP:-/tmp}"
data_dir="${SMOKE_DATA_DIR:-${runner_tmp}/mapflow-data}"
uploads_dir="${SMOKE_UPLOADS_DIR:-${runner_tmp}/mapflow-uploads}"
cookie_jar="${SMOKE_COOKIE_JAR:-${runner_tmp}/mapflow-smoke-cookie.txt}"
expected_b64_path="${EXPECTED_B64_PATH:-testdata/smoke/expected_sample_z0_x0_y0.mvt.base64}"

mkdir -p "$data_dir" "$uploads_dir"

cid=""
cleanup() {
  if [ -n "$cid" ]; then
    docker logs --tail 200 "$cid" || true
    docker stop "$cid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

cid=$(docker run -d --rm \
  -p "${port}:3000" \
  -e PORT=3000 \
  -e DB_PATH=/app/data/mapflow.duckdb \
  -e UPLOAD_DIR=/app/uploads \
  -v "${data_dir}:/app/data" \
  -v "${uploads_dir}:/app/uploads" \
  "${image}")

BASE_URL="http://127.0.0.1:${port}" \
FIXTURE_PATH="frontend/tests/fixtures/sample.geojson" \
TILE_Z=0 \
TILE_X=0 \
TILE_Y=0 \
OUT_PATH="/tmp/mapflow_smoke_tile.mvt" \
COOKIE_JAR="${cookie_jar}" \
EXPECTED_B64_PATH="${expected_b64_path}" \
  bash scripts/ci/fetch_tile.sh
