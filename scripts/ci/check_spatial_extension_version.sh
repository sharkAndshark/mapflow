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
import re
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
manifest_crate_version = manifest.get("duckdb_crate_version")
manifest_core_version = manifest.get("duckdb_core_version")
legacy_version = manifest.get("duckdb_version")

if not manifest_crate_version:
    manifest_crate_version = legacy_version
if not manifest_core_version:
    manifest_core_version = legacy_version

if not manifest_crate_version or not manifest_core_version:
    print(
        "manifest must define duckdb_crate_version/duckdb_core_version "
        "(or legacy duckdb_version)",
        file=sys.stderr,
    )
    sys.exit(1)

if manifest_crate_version != cargo_duckdb_version:
    print(
        "duckdb crate version mismatch: "
        f"Cargo.lock={cargo_duckdb_version}, manifest={manifest_crate_version}",
        file=sys.stderr,
    )
    sys.exit(1)

match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", cargo_duckdb_version)
if not match:
    print(
        f"unsupported duckdb crate version format in Cargo.lock: {cargo_duckdb_version}",
        file=sys.stderr,
    )
    sys.exit(1)

crate_major, crate_minor, crate_patch = map(int, match.groups())
if crate_major == 1 and crate_minor >= 10000:
    core_major = crate_minor // 10000
    core_minor = (crate_minor % 10000) // 100
    core_patch = crate_minor % 100
    expected_core_version = f"{core_major}.{core_minor}.{core_patch}"
else:
    # Pre-v1.5.0 duckdb-rs versions used direct DuckDB semver.
    expected_core_version = f"{crate_major}.{crate_minor}.{crate_patch}"

if manifest_core_version != expected_core_version:
    print(
        "duckdb core version mismatch: "
        f"derived={expected_core_version} from crate={cargo_duckdb_version}, "
        f"manifest={manifest_core_version}",
        file=sys.stderr,
    )
    sys.exit(1)

artifacts = manifest.get("artifacts")
if not isinstance(artifacts, list) or not artifacts:
    print("manifest must define at least one artifact", file=sys.stderr)
    sys.exit(1)

version_token = f"/v{manifest_core_version}/"
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
    f"(duckdb crate {cargo_duckdb_version}, core {manifest_core_version})"
)
PY
