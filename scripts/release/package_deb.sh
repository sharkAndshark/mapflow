#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <version> <artifact-id> <binary-path> <output-dir>" >&2
  exit 1
fi

version_raw="$1"
artifact_id="$2"
binary_path="$3"
output_dir="$4"

if [ ! -f "$binary_path" ]; then
  echo "binary not found: $binary_path" >&2
  exit 1
fi

case "$artifact_id" in
  linux-amd64) deb_arch="amd64" ;;
  linux-arm64) deb_arch="arm64" ;;
  *)
    echo "unsupported artifact-id for deb packaging: $artifact_id" >&2
    exit 1
    ;;
esac

normalize_version() {
  local v="$1"

  # Normalize stable tag like v0.1.2 -> 0.1.2
  if [[ "$v" =~ ^v[0-9] ]]; then
    v="${v#v}"
  fi

  if [[ ! "$v" =~ ^[A-Za-z0-9.+:~-]+$ ]]; then
    echo "invalid deb version: ${v}" >&2
    exit 1
  fi

  printf '%s' "$v"
}

deb_version="$(normalize_version "$version_raw")"
package_name="mapflow"
deb_name="${package_name}_${deb_version}-1_${deb_arch}.deb"
build_root="$(mktemp -d)"
package_root="${build_root}/${package_name}"
trap 'rm -rf "${build_root}"' EXIT

mkdir -p \
  "${package_root}/DEBIAN" \
  "${package_root}/usr/bin" \
  "${package_root}/usr/share/mapflow" \
  "${package_root}/usr/share/doc/${package_name}"

cp "$binary_path" "${package_root}/usr/bin/mapflow"
chmod 0755 "${package_root}/usr/bin/mapflow"
cp backend/extensions/spatial-extension-manifest.json "${package_root}/usr/share/mapflow/spatial-extension-manifest.json"
cp README.md "${package_root}/usr/share/doc/${package_name}/README.md"
cp LICENSE "${package_root}/usr/share/doc/${package_name}/LICENSE"
cp NOTICE "${package_root}/usr/share/doc/${package_name}/NOTICE"

cat > "${package_root}/DEBIAN/control" <<EOF
Package: ${package_name}
Version: ${deb_version}-1
Section: utils
Priority: optional
Architecture: ${deb_arch}
Maintainer: MapFlow Maintainers <maintainers@mapflow.invalid>
Description: MapFlow binary distribution
 Self-contained MapFlow server binary package.
EOF

mkdir -p "$output_dir"
archive_path="${output_dir}/${deb_name}"
dpkg-deb --build --root-owner-group "${package_root}" "${archive_path}" >/dev/null

echo "archive_path=${archive_path}"
