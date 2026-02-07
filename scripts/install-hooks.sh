#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"

git config core.hooksPath "$ROOT_DIR/.githooks"
echo "Installed git hooks path: $ROOT_DIR/.githooks"

chmod +x "$ROOT_DIR/.githooks/"* 2>/dev/null || true
echo "Ensured hook scripts are executable: $ROOT_DIR/.githooks/*"
