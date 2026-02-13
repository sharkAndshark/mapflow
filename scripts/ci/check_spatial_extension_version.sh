#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
LOCK_PATH="${ROOT_DIR}/Cargo.lock"
MANIFEST_PATH="${ROOT_DIR}/backend/extensions/spatial-extension-manifest.json"

if [ ! -f "${MANIFEST_PATH}" ]; then
  echo "spatial manifest not found: ${MANIFEST_PATH}" >&2
  exit 1
fi

python3 - "${LOCK_PATH}" "${MANIFEST_PATH}" <<'PY'
import json
import sys
import tomllib
from pathlib import Path

lock_path = Path(sys.argv[1])
manifest_path = Path(sys.argv[2])

lock_data = tomllib.loads(lock_path.read_text(encoding="utf-8"))
versions = sorted(
    {
        pkg.get("version")
        for pkg in lock_data.get("package", [])
        if pkg.get("name") == "duckdb" and pkg.get("version")
    }
)

if len(versions) != 1:
    print(
        f"expected exactly one duckdb version in Cargo.lock, found: {versions}",
        file=sys.stderr,
    )
    sys.exit(1)

cargo_duckdb_version = versions[0]
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
manifest_duckdb_version = manifest.get("duckdb_version")

if manifest_duckdb_version != cargo_duckdb_version:
    print(
        "duckdb version mismatch: "
        f"Cargo.lock={cargo_duckdb_version}, manifest={manifest_duckdb_version}",
        file=sys.stderr,
    )
    sys.exit(1)

artifacts = manifest.get("artifacts")
if not isinstance(artifacts, list) or not artifacts:
    print("manifest must define at least one artifact", file=sys.stderr)
    sys.exit(1)

version_token = f"/v{cargo_duckdb_version}/"
for idx, artifact in enumerate(artifacts):
    if not isinstance(artifact, dict):
        print(f"artifact[{idx}] must be an object", file=sys.stderr)
        sys.exit(1)

    platform = artifact.get("platform")
    archive_url = artifact.get("archive_url")
    local_relpath = artifact.get("local_relpath")

    if not platform or not archive_url or not local_relpath:
        print(
            f"artifact[{idx}] missing required fields (platform/archive_url/local_relpath)",
            file=sys.stderr,
        )
        sys.exit(1)

    if version_token not in archive_url:
        print(
            f"artifact[{idx}] URL does not contain {version_token}: {archive_url}",
            file=sys.stderr,
        )
        sys.exit(1)

    if version_token not in local_relpath:
        print(
            f"artifact[{idx}] local_relpath does not contain {version_token}: {local_relpath}",
            file=sys.stderr,
        )
        sys.exit(1)

print(
    "spatial extension manifest is in sync with Cargo.lock "
    f"(duckdb {cargo_duckdb_version})"
)
PY
