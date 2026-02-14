#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 5 ]; then
  echo "usage: $0 <version> <artifact-id> <binary-path> <extension-path> <output-dir>" >&2
  exit 1
fi

version="$1"
artifact_id="$2"
binary_path="$3"
extension_path="$4"
output_dir="$5"

if [ ! -f "$binary_path" ]; then
  echo "binary not found: $binary_path" >&2
  exit 1
fi

if [ ! -d "frontend/dist" ]; then
  echo "frontend bundle missing: frontend/dist" >&2
  exit 1
fi

if [ ! -f "$extension_path" ]; then
  echo "spatial extension not found: $extension_path" >&2
  exit 1
fi

bundle_name="mapflow-${version}-${artifact_id}"
bundle_dir="$(mktemp -d)/${bundle_name}"
mkdir -p "${bundle_dir}/extensions"

cp "$binary_path" "${bundle_dir}/mapflow"
chmod +x "${bundle_dir}/mapflow"
cp -R frontend/dist "${bundle_dir}/dist"
cp "$extension_path" "${bundle_dir}/extensions/spatial.duckdb_extension"
cp backend/extensions/spatial-extension-manifest.json "${bundle_dir}/spatial-extension-manifest.json"
cp README.md "${bundle_dir}/README.md"
cp LICENSE "${bundle_dir}/LICENSE"
cp NOTICE "${bundle_dir}/NOTICE"

mkdir -p "$output_dir"
archive_path="${output_dir}/${bundle_name}.tar.gz"
tar -C "$(dirname "${bundle_dir}")" -czf "${archive_path}" "${bundle_name}"

echo "archive_path=${archive_path}"
