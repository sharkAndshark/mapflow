#!/usr/bin/env bash

set -euo pipefail

SMOKE_USERNAME="${SMOKE_USERNAME:-smoke_admin}"
SMOKE_PASSWORD="${SMOKE_PASSWORD:-SmokePass1!}"

smoke_log() {
  echo "[smoke] $*" >&2
}

smoke_fail() {
  echo "[smoke] ERROR: $*" >&2
  exit 1
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
  init_state=$(curl -fsS "${base_url}/api/test/is-initialized" 2>/dev/null | \
    python3 -c 'import json,sys; print("true" if json.load(sys.stdin).get("initialized") else "false")' 2>/dev/null || echo "false")

  if [ "$init_state" = "false" ]; then
    smoke_log "initializing admin user"
    curl -fsS \
      -H "Content-Type: application/json" \
      -d "{\"username\":\"${SMOKE_USERNAME}\",\"password\":\"${SMOKE_PASSWORD}\"}" \
      "${base_url}/api/auth/init" >/dev/null
  fi
}

login() {
  local base_url="$1"
  local cookie_jar="$2"

  smoke_log "logging in"
  curl -fsS \
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
  resp=$(curl -fsS -b "$cookie_jar" -F "file=@${file_path}" "${base_url}/api/uploads")

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
  curl -fsS -b "$cookie_jar" -o "$out_path" "${base_url}/api/files/${file_id}/tiles/${z}/${x}/${y}"

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
  resp=$(curl -fsS -b "$cookie_jar" -X POST \
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
  curl -fsS -o "$out_path" "${base_url}/tiles/${slug}/${z}/${x}/${y}"

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
  sha=$(python3 -c "import hashlib; print(hashlib.sha256(open('${tile_path}','rb').read()).hexdigest())")
  smoke_log "tile sha256: ${sha}"

  if [ -n "$expected_b64_path" ] && [ -f "$expected_b64_path" ]; then
    python3 - "${tile_path}" "${expected_b64_path}" <<'PY'
import base64, hashlib, sys
tile_path, b64_path = sys.argv[1], sys.argv[2]
expected = base64.b64decode(open(b64_path).read().strip())
got = open(tile_path, 'rb').read()
if got != expected:
    print(f"tile mismatch: expected {hashlib.sha256(expected).hexdigest()}, got {hashlib.sha256(got).hexdigest()}")
    sys.exit(1)
print(f"tile verified: {hashlib.sha256(got).hexdigest()}")
PY
  fi
}
