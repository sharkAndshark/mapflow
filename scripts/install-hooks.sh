#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"

git config core.hooksPath "$ROOT_DIR/.githooks"
echo "Installed git hooks path: $ROOT_DIR/.githooks"
echo "Tip: if hooks don't run, ensure scripts are executable (chmod +x .githooks/*)"
