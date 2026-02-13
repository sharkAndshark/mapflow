#!/usr/bin/env bash

set -euo pipefail

base_url="${BASE_URL:-http://127.0.0.1:3000}"
fixture_path="${FIXTURE_PATH:-frontend/tests/fixtures/sample.geojson}"
z="${TILE_Z:-0}"
x="${TILE_X:-0}"
y="${TILE_Y:-0}"
out_path="${OUT_PATH:-/tmp/mapflow_smoke_tile.mvt}"
cookie_jar="${COOKIE_JAR:-/tmp/mapflow_smoke_cookie.txt}"
smoke_username="${SMOKE_USERNAME:-smoke_admin}"
smoke_password="${SMOKE_PASSWORD:-SmokePass1!}"
expected_b64_path="${EXPECTED_B64_PATH:-}"

auth_payload=$(
  SMOKE_USERNAME="$smoke_username" SMOKE_PASSWORD="$smoke_password" python3 - <<'PY'
import json
import os

print(json.dumps({
    "username": os.environ["SMOKE_USERNAME"],
    "password": os.environ["SMOKE_PASSWORD"],
}))
PY
)

wait_for_ready() {
  local tries="${1:-120}"
  local delay_s="${2:-0.5}"

  for i in $(seq 1 "$tries"); do
    if curl -fsS "${base_url}/api/test/is-initialized" >/dev/null; then
      return 0
    fi
    sleep "$delay_s"
  done

  echo "server did not become ready (${base_url})" >&2
  return 1
}

wait_for_file_status() {
  local id="$1"
  local want="$2"
  local tries="${3:-240}"
  local delay_s="${4:-0.5}"
  local files_json=""
  local st=""

  for i in $(seq 1 "$tries"); do
    if ! files_json=$(curl -fsS -b "$cookie_jar" "${base_url}/api/files" 2>/dev/null); then
      sleep "$delay_s"
      continue
    fi

    if ! st=$(FILES_JSON="$files_json" python3 -c 'import json,os,sys; id=sys.argv[1]; items=json.loads(os.environ.get("FILES_JSON", "[]")); print(next((it.get("status","") for it in items if it.get("id")==id), ""))' "$id" 2>/dev/null); then
      sleep "$delay_s"
      continue
    fi

    if [ "$st" = "$want" ]; then
      return 0
    fi
    if [ "$st" = "failed" ]; then
      echo "upload processing failed" >&2
      return 1
    fi
    sleep "$delay_s"
  done

  echo "timeout waiting for status=${want}" >&2
  return 1
}

initialize_if_needed() {
  local init_state
  init_state=$(
    curl -fsS "${base_url}/api/test/is-initialized" | python3 -c 'import json,sys; print("true" if json.load(sys.stdin).get("initialized") else "false")'
  )

  if [ "$init_state" = "false" ]; then
    curl -fsS \
      -H "Content-Type: application/json" \
      -d "$auth_payload" \
      "${base_url}/api/auth/init" >/dev/null
  fi
}

login() {
  curl -fsS \
    -c "$cookie_jar" \
    -H "Content-Type: application/json" \
    -d "$auth_payload" \
    "${base_url}/api/auth/login" >/dev/null
}

wait_for_ready
initialize_if_needed
login

curl -fsS -b "$cookie_jar" "${base_url}/api/files" >/dev/null

upload_resp=$(curl -fsS -b "$cookie_jar" -F "file=@${fixture_path}" "${base_url}/api/uploads")
id=$(python3 -c 'import json,sys; print(json.loads(sys.stdin.read())["id"])' <<<"$upload_resp")

wait_for_file_status "$id" ready

curl -fsS -b "$cookie_jar" "${base_url}/api/files/${id}/preview" >/dev/null
curl -fsS -b "$cookie_jar" -o "$out_path" "${base_url}/api/files/${id}/tiles/${z}/${x}/${y}"

python3 - <<PY
import hashlib
p='${out_path}'
b=open(p,'rb').read()
print('tile_bytes',len(b))
print('sha256',hashlib.sha256(b).hexdigest())
PY

if [ -n "$expected_b64_path" ]; then
  EXPECTED_B64_PATH="$expected_b64_path" OUT_PATH="$out_path" python3 - <<'PY'
import base64
import hashlib
import os

expected_b64 = open(os.environ["EXPECTED_B64_PATH"], "r", encoding="ascii").read().strip()
expected = base64.b64decode(expected_b64)
got = open(os.environ["OUT_PATH"], "rb").read()

if got != expected:
    print("tile mismatch")
    print("expected_sha256", hashlib.sha256(expected).hexdigest())
    print("got_sha256", hashlib.sha256(got).hexdigest())
    raise SystemExit(1)

print("tile ok sha256", hashlib.sha256(got).hexdigest())
PY
fi
