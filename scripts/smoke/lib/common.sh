#!/usr/bin/env bash

set -euo pipefail

SMOKE_USERNAME="${SMOKE_USERNAME:-smoke_admin}"
SMOKE_PASSWORD="${SMOKE_PASSWORD:-SmokePass1!}"
SMOKE_HTTP_RETRIES="${SMOKE_HTTP_RETRIES:-3}"
SMOKE_HTTP_RETRY_DELAY="${SMOKE_HTTP_RETRY_DELAY:-0.5}"

smoke_log() {
  echo "[smoke] $*" >&2
}

smoke_fail() {
  echo "[smoke] ERROR: $*" >&2
  exit 1
}

curl_with_retry() {
  local max_retries="$SMOKE_HTTP_RETRIES"
  local delay="$SMOKE_HTTP_RETRY_DELAY"
  local attempt=1
  local exit_code=0

  while true; do
    if curl "$@"; then
      return 0
    else
      exit_code=$?
    fi

    if [ "$attempt" -ge "$max_retries" ]; then
      return "$exit_code"
    fi

    smoke_log "curl failed (attempt ${attempt}/${max_retries}, code=${exit_code}), retrying..."
    sleep "$delay"
    attempt=$((attempt + 1))
  done
}

wait_for_ready() {
  local base_url="$1"
  local max_tries="${2:-120}"
  local delay="${3:-0.5}"

  for i in $(seq 1 "$max_tries"); do
    if curl -fsS "${base_url}/health" >/dev/null 2>&1; then
      smoke_log "server ready (${base_url})"
      return 0
    fi
    sleep "$delay"
  done

  smoke_fail "server not ready after ${max_tries} tries (${base_url})"
}

init_if_needed() {
  local base_url="$1"
  local cookie_jar="$2"

  local init_state
  init_state=$(curl_with_retry -fsS "${base_url}/api/test/is-initialized" 2>/dev/null | \
    python3 -c 'import json,sys; print("true" if json.load(sys.stdin).get("initialized") else "false")' 2>/dev/null || echo "false")

  if [ "$init_state" = "false" ]; then
    smoke_log "initializing admin user"
    curl_with_retry -fsS \
      -H "Content-Type: application/json" \
      -d "{\"username\":\"${SMOKE_USERNAME}\",\"password\":\"${SMOKE_PASSWORD}\"}" \
      "${base_url}/api/auth/init" >/dev/null
  fi
}

login() {
  local base_url="$1"
  local cookie_jar="$2"

  smoke_log "logging in"
  curl_with_retry -fsS \
    -c "$cookie_jar" \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"${SMOKE_USERNAME}\",\"password\":\"${SMOKE_PASSWORD}\"}" \
    "${base_url}/api/auth/login" >/dev/null
}

upload_file() {
  local base_url="$1"
  local cookie_jar="$2"
  local file_path="$3"

  smoke_log "uploading ${file_path}"
  local resp
  resp=$(curl_with_retry -fsS -b "$cookie_jar" -F "file=@${file_path}" "${base_url}/api/uploads")

  echo "$resp" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'
}

wait_for_status() {
  local base_url="$1"
  local cookie_jar="$2"
  local file_id="$3"
  local target_status="${4:-ready}"
  local max_tries="${5:-240}"
  local delay="${6:-0.5}"

  for i in $(seq 1 "$max_tries"); do
    local files_json status
    files_json=$(curl -fsS -b "$cookie_jar" "${base_url}/api/files" 2>/dev/null) || {
      sleep "$delay"
      continue
    }

    status=$(echo "$files_json" | python3 -c "
import json,sys
fid='$file_id'
for it in json.load(sys.stdin):
    if it.get('id')==fid:
        print(it.get('status',''))
        break
" 2>/dev/null) || {
      sleep "$delay"
      continue
    }

    if [ "$status" = "$target_status" ]; then
      smoke_log "file ${file_id} status=${target_status}"
      return 0
    fi

    if [ "$status" = "failed" ]; then
      smoke_fail "file ${file_id} processing failed"
    fi

    sleep "$delay"
  done

  smoke_fail "timeout waiting for file ${file_id} status=${target_status}"
}

get_tile() {
  local base_url="$1"
  local cookie_jar="$2"
  local file_id="$3"
  local z="${4:-0}"
  local x="${5:-0}"
  local y="${6:-0}"
  local out_path="$7"

  smoke_log "fetching tile ${z}/${x}/${y}"
  curl_with_retry -fsS -b "$cookie_jar" -o "$out_path" "${base_url}/api/files/${file_id}/tiles/${z}/${x}/${y}"

  local size
  size=$(wc -c < "$out_path" | tr -d ' ')
  smoke_log "tile size: ${size} bytes"
}

publish_file() {
  local base_url="$1"
  local cookie_jar="$2"
  local file_id="$3"

  smoke_log "publishing file ${file_id}"
  local resp
  resp=$(curl_with_retry -fsS -b "$cookie_jar" -X POST \
    -H "Content-Type: application/json" \
    -d "{}" \
    "${base_url}/api/files/${file_id}/publish")

  echo "$resp" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("slug",""))'
}

get_public_tile() {
  local base_url="$1"
  local slug="$2"
  local z="${3:-0}"
  local x="${4:-0}"
  local y="${5:-0}"
  local out_path="$6"

  smoke_log "fetching public tile ${slug}/${z}/${x}/${y}"
  curl_with_retry -fsS -o "$out_path" "${base_url}/tiles/${slug}/${z}/${x}/${y}"

  local size
  size=$(wc -c < "$out_path" | tr -d ' ')
  smoke_log "public tile size: ${size} bytes"
}

verify_tile_content() {
  local tile_path="$1"
  local expected_b64_path="${2:-}"

  if [ ! -f "$tile_path" ]; then
    smoke_fail "tile file not found: ${tile_path}"
  fi

  local sha
  sha=$(python3 -c "import hashlib,sys; print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest())" < "$tile_path")
  smoke_log "tile sha256: ${sha}"

  if [ -n "$expected_b64_path" ] && [ -f "$expected_b64_path" ]; then
    python3 -c "import base64,hashlib,sys; expected=base64.b64decode(open(sys.argv[1],'rb').read().strip()); got=sys.stdin.buffer.read(); mismatch=(got!=expected); print(f'tile mismatch: expected {hashlib.sha256(expected).hexdigest()}, got {hashlib.sha256(got).hexdigest()}' if mismatch else f'tile verified: {hashlib.sha256(got).hexdigest()}'); sys.exit(1 if mismatch else 0)" \
      "$expected_b64_path" < "$tile_path"
  fi
}

verify_invalid_upload_rejected() {
  local base_url="$1"
  local cookie_jar="$2"
  local file_path="$3"

  smoke_log "verifying invalid upload is rejected: ${file_path}"

  local response_file
  response_file="$(mktemp)"

  local http_code
  http_code=$(curl -sS -o "$response_file" -w "%{http_code}" -b "$cookie_jar" \
    -F "file=@${file_path}" "${base_url}/api/uploads" || true)

  if [ "$http_code" != "400" ]; then
    local body
    body="$(cat "$response_file" 2>/dev/null || true)"
    rm -f "$response_file"
    smoke_fail "expected invalid upload status 400, got ${http_code}, body=${body}"
  fi

  if ! grep -q "Unsupported file type" "$response_file"; then
    local body
    body="$(cat "$response_file" 2>/dev/null || true)"
    rm -f "$response_file"
    smoke_fail "invalid upload error mismatch: ${body}"
  fi

  rm -f "$response_file"
  smoke_log "invalid upload rejected as expected"
}

set_upload_max_size_mb() {
  local base_url="$1"
  local cookie_jar="$2"
  local max_size_mb="$3"

  smoke_log "setting upload max size to ${max_size_mb} MB"

  local response_file
  response_file="$(mktemp)"

  local http_code
  http_code=$(curl -sS -o "$response_file" -w "%{http_code}" -b "$cookie_jar" -X PATCH \
    -H "Content-Type: application/json" \
    -d "{\"maxSizeMb\":${max_size_mb}}" \
    "${base_url}/api/settings" || true)

  if [ "$http_code" != "200" ]; then
    local body
    body="$(cat "$response_file" 2>/dev/null || true)"
    rm -f "$response_file"
    smoke_fail "failed to update upload max size, status=${http_code}, body=${body}"
  fi

  local returned_mb
  returned_mb=$(python3 -c 'import json,sys; print(json.load(sys.stdin).get("maxSizeMb",""))' < "$response_file" 2>/dev/null || true)

  rm -f "$response_file"

  if [ "$returned_mb" != "$max_size_mb" ]; then
    smoke_fail "upload max size mismatch after update: expected ${max_size_mb}, got ${returned_mb}"
  fi

  smoke_log "upload max size updated"
}

verify_schema_endpoint() {
  local base_url="$1"
  local cookie_jar="$2"
  local file_id="$3"

  smoke_log "verifying schema endpoint for file ${file_id}"

  local response_file
  response_file="$(mktemp)"

  local http_code
  http_code=$(curl -sS -o "$response_file" -w "%{http_code}" -b "$cookie_jar" \
    "${base_url}/api/files/${file_id}/schema" || true)

  if [ "$http_code" != "200" ]; then
    local body
    body="$(cat "$response_file" 2>/dev/null || true)"
    rm -f "$response_file"
    smoke_fail "expected schema status 200, got ${http_code}, body=${body}"
  fi

  python3 - "$response_file" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

layers = data.get("layers")
if not isinstance(layers, list):
    raise SystemExit("schema validation failed: layers is not an array")

if len(layers) == 0:
    raise SystemExit("schema validation failed: layers is empty")

first = layers[0]
if not isinstance(first, dict):
    raise SystemExit("schema validation failed: first layer is not an object")

fields = first.get("fields")
if not isinstance(fields, list):
    raise SystemExit("schema validation failed: fields is not an array")

if len(fields) == 0:
    raise SystemExit("schema validation failed: fields is empty")
PY

  rm -f "$response_file"
  smoke_log "schema endpoint verified"
}

verify_oversize_upload_rejected() {
  local base_url="$1"
  local cookie_jar="$2"
  local file_path="$3"

  smoke_log "verifying oversize upload is rejected: ${file_path}"

  local response_file
  response_file="$(mktemp)"

  local http_code
  http_code=$(curl -sS -o "$response_file" -w "%{http_code}" -b "$cookie_jar" \
    -F "file=@${file_path}" "${base_url}/api/uploads" || true)

  if [ "$http_code" != "413" ]; then
    local body
    body="$(cat "$response_file" 2>/dev/null || true)"
    rm -f "$response_file"
    smoke_fail "expected oversize upload status 413, got ${http_code}, body=${body}"
  fi

  if ! grep -q "File too large" "$response_file"; then
    local body
    body="$(cat "$response_file" 2>/dev/null || true)"
    rm -f "$response_file"
    smoke_fail "oversize upload error mismatch: ${body}"
  fi

  rm -f "$response_file"
  smoke_log "oversize upload rejected as expected"
}
