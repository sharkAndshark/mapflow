#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <artifact-id> <binary-path> <output-dir>" >&2
  exit 1
fi

version="$1"
artifact_id="$2"
binary_path="$3"
output_dir="$4"

if [ ! -f "$binary_path" ]; then
  echo "binary not found: $binary_path" >&2
  exit 1
fi

if [ ! -d "frontend/dist" ]; then
  echo "frontend bundle missing: frontend/dist" >&2
  exit 1
fi

bundle_name="mapflow-${version}-${artifact_id}"
bundle_dir="$(mktemp -d)/${bundle_name}"
mkdir -p "${bundle_dir}"

cp "$binary_path" "${bundle_dir}/mapflow"
chmod +x "${bundle_dir}/mapflow"
cp -R frontend/dist "${bundle_dir}/dist"
cp backend/extensions/spatial-extension-manifest.json "${bundle_dir}/spatial-extension-manifest.json"
cp README.md "${bundle_dir}/README.md"
cp LICENSE "${bundle_dir}/LICENSE"
cp NOTICE "${bundle_dir}/NOTICE"

mkdir -p "$output_dir"
archive_path="${output_dir}/${bundle_name}.tar.gz"
tar -C "$(dirname "${bundle_dir}")" -czf "${archive_path}" "${bundle_name}"

echo "archive_path=${archive_path}"
