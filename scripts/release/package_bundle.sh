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

if [ ! -f "$extension_path" ]; then
  echo "spatial extension not found: $extension_path" >&2
  exit 1
fi

# Detect if this is a Windows platform based on artifact_id
is_windows=false
if [[ "$artifact_id" == *"windows"* ]] || [[ "$artifact_id" == *"win"* ]]; then
  is_windows=true
fi

bundle_name="mapflow-${version}-${artifact_id}"
bundle_dir="$(mktemp -d)/${bundle_name}"
mkdir -p "${bundle_dir}/extensions"

# Copy binary with appropriate name and extension
if [ "$is_windows" = true ]; then
  cp "$binary_path" "${bundle_dir}/mapflow.exe"
else
  cp "$binary_path" "${bundle_dir}/mapflow"
  chmod +x "${bundle_dir}/mapflow"
fi
cp "$extension_path" "${bundle_dir}/extensions/spatial.duckdb_extension"
cp backend/extensions/spatial-extension-manifest.json "${bundle_dir}/spatial-extension-manifest.json"
cp README.md "${bundle_dir}/README.md"
cp LICENSE "${bundle_dir}/LICENSE"
cp NOTICE "${bundle_dir}/NOTICE"

mkdir -p "$output_dir"

# Create archive with appropriate format
if [ "$is_windows" = true ]; then
  archive_path="${output_dir}/${bundle_name}.zip"
  # Use zip for Windows; -r for recursive, -q for quiet
  (cd "$(dirname "${bundle_dir}")" && zip -r -q "${archive_path}" "${bundle_name}")
else
  archive_path="${output_dir}/${bundle_name}.tar.gz"
  tar -C "$(dirname "${bundle_dir}")" -czf "${archive_path}" "${bundle_name}"
fi

echo "archive_path=${archive_path}"
