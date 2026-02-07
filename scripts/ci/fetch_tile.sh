#!/usr/bin/env bash

set -euo pipefail

base_url="${BASE_URL:-http://127.0.0.1:3000}"
fixture_path="${FIXTURE_PATH:-frontend/tests/fixtures/sample.geojson}"
z="${TILE_Z:-0}"
x="${TILE_X:-0}"
y="${TILE_Y:-0}"
out_path="${OUT_PATH:-/tmp/mapflow_smoke_tile.mvt}"

wait_for_ready() {
  local tries="${1:-120}"
  local delay_s="${2:-0.5}"

  for i in $(seq 1 "$tries"); do
    if curl -fsS "${base_url}/api/files" >/dev/null; then
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

  for i in $(seq 1 "$tries"); do
    st=$(curl -fsS "${base_url}/api/files" | python3 -c 'import json,sys; id=sys.argv[1]; want=sys.argv[2]; items=json.load(sys.stdin); st=next((it.get("status","") for it in items if it.get("id")==id), ""); print(st)' "$id" "$want")
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

wait_for_ready

upload_resp=$(curl -fsS -F "file=@${fixture_path}" "${base_url}/api/uploads")
id=$(python3 -c 'import json,sys; print(json.loads(sys.stdin.read())["id"])' <<<"$upload_resp")

wait_for_file_status "$id" ready

curl -fsS -o "$out_path" "${base_url}/api/files/${id}/tiles/${z}/${x}/${y}"

python3 - <<PY
import hashlib
p='${out_path}'
b=open(p,'rb').read()
print('tile_bytes',len(b))
print('sha256',hashlib.sha256(b).hexdigest())
PY
