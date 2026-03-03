#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage:" >&2
  echo "  $0 url <platform>" >&2
  echo "  $0 download <platform> <output-path>" >&2
  exit 1
fi

mode="$1"
platform="$2"
manifest_path="backend/extensions/spatial-extension-manifest.json"

if [ ! -f "$manifest_path" ]; then
  echo "manifest not found: $manifest_path" >&2
  exit 1
fi

artifact_data="$(
  python3 - "$manifest_path" "$platform" "$mode" <<'PY'
import json
import sys

manifest_path = sys.argv[1]
platform = sys.argv[2]
mode = sys.argv[3]

manifest = json.load(open(manifest_path, encoding="utf-8"))
artifacts = manifest.get("artifacts", [])

for artifact in artifacts:
    if artifact.get("platform") == platform:
        url = artifact.get("archive_url")
        if not url:
            print(f"artifact '{platform}' missing archive_url", file=sys.stderr)
            raise SystemExit(1)
        if mode == "download":
            sha256 = artifact.get("archive_sha256")
            if not sha256:
                print(f"artifact '{platform}' missing archive_sha256", file=sys.stderr)
                raise SystemExit(1)
            print(f"{url}|{sha256}")
        else:
            print(url)
        raise SystemExit(0)

print(f"platform not found in manifest: {platform}", file=sys.stderr)
raise SystemExit(1)
PY
)"

compute_sha256() {
  local file_path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file_path" | awk '{print $1}'
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file_path" | awk '{print $1}'
    return
  fi

  if command -v python3 >/dev/null 2>&1; then
    python3 - "$file_path" <<'PY'
import hashlib
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
print(hashlib.sha256(path.read_bytes()).hexdigest())
PY
    return
  fi

  if command -v python >/dev/null 2>&1; then
    python - "$file_path" <<'PY'
import hashlib
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
print(hashlib.sha256(path.read_bytes()).hexdigest())
PY
    return
  fi

  echo "no sha256 tool available (sha256sum/shasum/python3/python)" >&2
  exit 1
}

case "$mode" in
  url)
    echo "$artifact_data"
    ;;
  download)
    if [ "$#" -ne 3 ]; then
      echo "usage: $0 download <platform> <output-path>" >&2
      exit 1
    fi

    output_path="$3"
    output_dir="$(dirname "$output_path")"
    mkdir -p "$output_dir"

    IFS='|' read -r archive_url archive_sha256 <<< "$artifact_data"

    tmp_gz="$(mktemp)"
    curl -fsSL "$archive_url" -o "$tmp_gz"

    actual_sha256="$(compute_sha256 "$tmp_gz")"
    if [ "$actual_sha256" != "$archive_sha256" ]; then
      echo "sha256 mismatch for ${platform}: expected ${archive_sha256}, got ${actual_sha256}" >&2
      rm -f "$tmp_gz"
      exit 1
    fi

    gunzip -c "$tmp_gz" > "$output_path"
    rm -f "$tmp_gz"
    chmod 0644 "$output_path"
    ;;
  *)
    echo "unsupported mode: $mode" >&2
    exit 1
    ;;
esac
