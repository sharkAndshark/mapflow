#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Generate Homebrew formula for MapFlow preview channel from a GitHub release tag.

Usage:
  scripts/release/generate_homebrew_formula.sh \
    --tag <release-tag> \
    [--repo <owner/repo>] \
    [--formula-name <formula-name>] \
    [--class-name <ruby-class-name>] \
    [--output <output-path>]

Example:
  scripts/release/generate_homebrew_formula.sh \
    --tag nightly-20260305-df8780e-r52 \
    --repo sharkAndshark/mapflow \
    --output /tmp/mapflow-preview.rb
EOF
}

REPO="sharkAndshark/mapflow"
FORMULA_NAME="mapflow-preview"
CLASS_NAME="MapflowPreview"
TAG=""
OUTPUT=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo)
      REPO="${2:-}"
      shift 2
      ;;
    --tag)
      TAG="${2:-}"
      shift 2
      ;;
    --formula-name)
      FORMULA_NAME="${2:-}"
      shift 2
      ;;
    --class-name)
      CLASS_NAME="${2:-}"
      shift 2
      ;;
    --output)
      OUTPUT="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [ -z "$TAG" ]; then
  echo "--tag is required" >&2
  usage >&2
  exit 1
fi

if [ -z "$OUTPUT" ]; then
  OUTPUT="Formula/${FORMULA_NAME}.rb"
fi

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI is required" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

release_json="$(gh release view "$TAG" --repo "$REPO" --json tagName,assets)"
tag_name="$(printf '%s' "$release_json" | jq -r '.tagName')"
if [ -z "$tag_name" ] || [ "$tag_name" = "null" ]; then
  echo "failed to resolve release tag for $TAG" >&2
  exit 1
fi
version="${tag_name#v}"

find_asset() {
  local pattern="$1"
  printf '%s' "$release_json" | jq -r --arg pattern "$pattern" '
    .assets[]
    | select(.name | test($pattern))
    | [.url, (.digest // "")]
    | @tsv
  ' | head -n 1
}

darwin_line="$(find_asset "darwin-arm64\\.tar\\.gz$")""
linux_amd64_line="$(find_asset "linux-amd64\\.tar\\.gz$")"
linux_arm64_line="$(find_asset "linux-arm64\\.tar\\.gz$")"

if [ -z "$darwin_line" ] || [ -z "$linux_amd64_line" ] || [ -z "$linux_arm64_line" ]; then
  echo "missing required assets (darwin-arm64/linux-amd64/linux-arm64) for tag: $tag_name" >&2
  exit 1
fi

IFS=$'\t' read -r darwin_url darwin_digest <<<"$darwin_line"
IFS=$'\t' read -r linux_amd64_url linux_amd64_digest <<<"$linux_amd64_line"
IFS=$'\t' read -r linux_arm64_url linux_arm64_digest <<<"$linux_arm64_line"

normalize_sha() {
  local digest="$1"
  if [ -z "$digest" ] || [ "$digest" = "null" ]; then
    return 1
  fi
  if [[ "$digest" == sha256:* ]]; then
    printf '%s' "${digest#sha256:}"
    return 0
  fi
  printf '%s' "$digest"
  return 0
}

if ! darwin_sha="$(normalize_sha "$darwin_digest")"; then
  echo "missing digest for darwin-arm64 asset" >&2
  exit 1
fi
if ! linux_amd64_sha="$(normalize_sha "$linux_amd64_digest")"; then
  echo "missing digest for linux-amd64 asset" >&2
  exit 1
fi
if ! linux_arm64_sha="$(normalize_sha "$linux_arm64_digest")"; then
  echo "missing digest for linux-arm64 asset" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"
cat >"$OUTPUT" <<EOF
class ${CLASS_NAME} < Formula
  desc "MapFlow early preview build (breaking changes allowed)"
  homepage "https://github.com/${REPO}"
  license "Apache-2.0"
  version "${version}"

  on_macos do
    if Hardware::CPU.arm?
      url "${darwin_url}"
      sha256 "${darwin_sha}"
    else
      odie "mapflow-preview currently provides macOS arm64 builds only"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "${linux_arm64_url}"
      sha256 "${linux_arm64_sha}"
    elsif Hardware::CPU.intel?
      url "${linux_amd64_url}"
      sha256 "${linux_amd64_sha}"
    else
      odie "unsupported Linux CPU architecture for mapflow-preview"
    end
  end

  def install
    staged_dir_name = Dir.children(buildpath).find do |entry|
      (buildpath/entry).directory? && entry.start_with?("mapflow-")
    end
    staged = staged_dir_name ? (buildpath/staged_dir_name) : buildpath

    bin.install staged/"mapflow"

    %w[README.md LICENSE NOTICE spatial-extension-manifest.json].each do |filename|
      candidate = staged/filename
      pkgshare.install candidate if candidate.exist?
    end

    macos_scripts_dir = staged/"macos"
    if macos_scripts_dir.directory?
      (pkgshare/"macos").install Dir[macos_scripts_dir/"*"]
    end
  end

  def caveats
    <<~EOS
      This formula installs an early preview build of MapFlow.
      Breaking changes may be introduced at any time.
      Upgrades are NOT guaranteed to be backward compatible.

      Before upgrading, back up your data directory:
        ~/.mapflow
    EOS
  end

  test do
    output = shell_output("#{bin}/mapflow --help 2>&1")
    assert_match(/USAGE|Usage/, output)
  end
end
EOF

echo "generated: $OUTPUT"
echo "tag: $tag_name"
echo "version: $version"
