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

archive_url="$(
  python3 - "$manifest_path" "$platform" <<'PY'
import json
import sys

manifest_path = sys.argv[1]
platform = sys.argv[2]

manifest = json.load(open(manifest_path, encoding="utf-8"))
artifacts = manifest.get("artifacts", [])

for artifact in artifacts:
    if artifact.get("platform") == platform:
        url = artifact.get("archive_url")
        if not url:
            print(f"artifact '{platform}' missing archive_url", file=sys.stderr)
            raise SystemExit(1)
        print(url)
        raise SystemExit(0)

print(f"platform not found in manifest: {platform}", file=sys.stderr)
raise SystemExit(1)
PY
)"

case "$mode" in
  url)
    echo "$archive_url"
    ;;
  download)
    if [ "$#" -ne 3 ]; then
      echo "usage: $0 download <platform> <output-path>" >&2
      exit 1
    fi

    output_path="$3"
    output_dir="$(dirname "$output_path")"
    mkdir -p "$output_dir"

    tmp_gz="$(mktemp)"
    curl -fsSL "$archive_url" -o "$tmp_gz"
    gunzip -c "$tmp_gz" > "$output_path"
    rm -f "$tmp_gz"
    chmod 0644 "$output_path"
    ;;
  *)
    echo "unsupported mode: $mode" >&2
    exit 1
    ;;
esac
