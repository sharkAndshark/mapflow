#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <duckdb-crate-version>" >&2
  exit 1
fi

crate_version="$1"

if ! [[ "$crate_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid duckdb crate version: ${crate_version}" >&2
  echo "expected format: X.Y.Z" >&2
  exit 1
fi

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo_toml="${root_dir}/backend/Cargo.toml"
manifest_path="${root_dir}/backend/extensions/spatial-extension-manifest.json"

python3 - "${cargo_toml}" "${crate_version}" <<'PY'
import re
import sys
from pathlib import Path

cargo_toml = Path(sys.argv[1])
crate_version = sys.argv[2]
text = cargo_toml.read_text(encoding="utf-8")
pattern = r'(duckdb\s*=\s*\{\s*version\s*=\s*")([0-9]+\.[0-9]+\.[0-9]+)(")'
new_text, count = re.subn(pattern, rf"\g<1>{crate_version}\3", text, count=1)
if count != 1:
    raise SystemExit("failed to locate duckdb dependency version in backend/Cargo.toml")
cargo_toml.write_text(new_text, encoding="utf-8")
PY

cargo update \
  --manifest-path "${root_dir}/backend/Cargo.toml" \
  -p duckdb \
  --precise "${crate_version}"

python3 - "${manifest_path}" "${crate_version}" <<'PY'
import json
import re
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
crate_version = sys.argv[2]
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
extension_name = manifest.get("extension_name", "spatial")

match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", crate_version)
if not match:
    raise SystemExit(f"invalid crate version: {crate_version}")

crate_major, crate_minor, crate_patch = map(int, match.groups())
if crate_major == 1 and crate_minor >= 10000:
    core_major = crate_minor // 10000
    core_minor = (crate_minor % 10000) // 100
    core_patch = crate_minor % 100
    core_version = f"{core_major}.{core_minor}.{core_patch}"
else:
    # Pre-v1.5.0 duckdb-rs versions used direct DuckDB semver.
    core_version = f"{crate_major}.{crate_minor}.{crate_patch}"

manifest["duckdb_crate_version"] = crate_version
manifest["duckdb_core_version"] = core_version
# Backward-compatible key used by some packaging scripts.
manifest["duckdb_version"] = core_version
for artifact in manifest.get("artifacts", []):
    platform = artifact.get("platform")
    if not platform:
        continue
    artifact["archive_url"] = (
        f"http://extensions.duckdb.org/v{core_version}/{platform}/"
        f"{extension_name}.duckdb_extension.gz"
    )
    artifact["archive_sha256"] = ""
    artifact["local_relpath"] = (
        f"backend/extensions/duckdb/v{core_version}/{platform}/"
        f"{extension_name}.duckdb_extension"
    )

manifest_path.write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
PY

bash "${root_dir}/scripts/ci/check_spatial_extension_version.sh"

core_version="$(
  python3 - "${manifest_path}" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
print(manifest.get("duckdb_core_version", manifest.get("duckdb_version", "")))
PY
)"

echo "duckdb upgraded to crate ${crate_version} (core ${core_version})"
