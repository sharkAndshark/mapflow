#!/usr/bin/env bash

set -euo pipefail

DOC_PATH="${1:-docs/dev/behaviors.md}"

if [ ! -f "$DOC_PATH" ]; then
  echo "ERROR: behaviors doc not found: $DOC_PATH" >&2
  exit 1
fi

backend_tests_file="$(mktemp)"
trap 'rm -f "$backend_tests_file"' EXIT

cargo test --manifest-path backend/Cargo.toml -- --list 2>/dev/null \
  | sed -n 's/: test$//p' \
  > "$backend_tests_file"

refs="$(
  awk -F'|' '/^\| [A-Z0-9-]+ / {print $6}' "$DOC_PATH" \
    | grep -oE '`[^`]+`' \
    | tr -d '`' \
    || true
)"

if [ -z "$refs" ]; then
  echo "ERROR: no backtick references found in $DOC_PATH" >&2
  exit 1
fi

errors=0

while IFS= read -r ref; do
  ref="$(echo "$ref" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  [ -z "$ref" ] && continue

  if [[ "$ref" == cargo\ test* ]]; then
    filters="$(echo "$ref" | grep -oE 'test_[A-Za-z0-9_*]+' || true)"
    if [ -z "$filters" ]; then
      echo "WARN: no test filter found in cargo command: $ref" >&2
      continue
    fi

    for filter in $filters; do
      regex="${filter//\*/.*}"
      if ! grep -Eq "(^|::)${regex}$" "$backend_tests_file"; then
        echo "ERROR: missing Rust test filter '$filter' (from: $ref)" >&2
        errors=1
      fi
    done
    continue
  fi

  # Non-cargo refs: validate file paths that appear in the command/reference.
  for raw_token in $ref; do
    token="${raw_token%,}"
    token="${token%)}"
    token="${token#(}"

    # Support path::test_name notation.
    file_token="${token%%::*}"

    if [[ "$file_token" =~ ^[A-Za-z0-9._/-]+\.(js|ts|jsx|tsx|yml|yaml|sh|md|rs)$ ]]; then
      if [ ! -f "$file_token" ]; then
        echo "ERROR: missing file reference '$file_token' (from: $ref)" >&2
        errors=1
      fi
    fi
  done
done <<< "$refs"

if [ "$errors" -ne 0 ]; then
  exit 1
fi

echo "behaviors references lint passed"
