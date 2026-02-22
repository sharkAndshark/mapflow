#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <duckdb-version>" >&2
  exit 1
fi

version="$1"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "invalid duckdb version: ${version}" >&2
  echo "expected format: X.Y.Z" >&2
  exit 1
fi

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo_toml="${root_dir}/backend/Cargo.toml"
manifest_path="${root_dir}/backend/extensions/spatial-extension-manifest.json"

python3 - "${cargo_toml}" "${version}" <<'PY'
import re
import sys
from pathlib import Path

cargo_toml = Path(sys.argv[1])
version = sys.argv[2]
text = cargo_toml.read_text(encoding="utf-8")
pattern = r'(duckdb\s*=\s*\{\s*version\s*=\s*")([0-9]+\.[0-9]+\.[0-9]+)(")'
new_text, count = re.subn(pattern, rf"\g<1>{version}\3", text, count=1)
if count != 1:
    raise SystemExit("failed to locate duckdb dependency version in backend/Cargo.toml")
cargo_toml.write_text(new_text, encoding="utf-8")
PY

cargo update \
  --manifest-path "${root_dir}/backend/Cargo.toml" \
  -p duckdb \
  --precise "${version}"

python3 - "${manifest_path}" "${version}" <<'PY'
import json
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
version = sys.argv[2]
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
extension_name = manifest.get("extension_name", "spatial")

manifest["duckdb_version"] = version
for artifact in manifest.get("artifacts", []):
    platform = artifact.get("platform")
    if not platform:
        continue
    artifact["archive_url"] = (
        f"http://extensions.duckdb.org/v{version}/{platform}/"
        f"{extension_name}.duckdb_extension.gz"
    )
    artifact["local_relpath"] = (
        f"backend/extensions/duckdb/v{version}/{platform}/"
        f"{extension_name}.duckdb_extension"
    )

manifest_path.write_text(
    json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
PY

bash "${root_dir}/scripts/ci/check_spatial_extension_version.sh"

echo "duckdb upgraded to ${version}"
