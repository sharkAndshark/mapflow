#!/usr/bin/env bash

set -euo pipefail

# Configuration
BASE_URL="${BASE_URL:-http://127.0.0.1:3000}"
OUTPUT_DIR="${OUTPUT_DIR:-testdata/smoke}"
GENERATE_MODE="${GENERATE_MODE:-0}"

# Datasets to generate goldens for
DATASETS=(
  "sf_lines:testdata/osm_medium/geojson/sf_lines.geojson"
  "sf_points:testdata/osm_medium/geojson/sf_points.geojson"
  "sf_polygons:testdata/osm_medium/geojson/sf_polygons.geojson"
)

# Tile coordinates for each zoom level (hit/empty/boundary)
# Format: z:x_hit:y_hit:x_empty:y_empty:x_boundary:y_boundary
TILE_COORDS=(
  "0:0:0:0:0:0:0"
  "1:0:0:1:1:0:0"
  "2:0:1:3:3:0:1"
  "3:1:3:7:7:1:3"
  "4:2:6:15:15:2:6"
  "5:5:12:31:31:5:12"
  "6:10:24:63:63:10:24"
  "7:20:49:120:127:20:49"
  "8:40:98:140:198:40:98"
  "9:81:197:181:297:81:197"
  "10:163:395:263:495:163:395"
  "11:327:791:427:891:327:791"
  "12:655:1583:755:1683:654:1583"
  "13:1310:3166:1410:3266:1309:3166"
  "14:2620:6332:2720:6432:2619:6333"
)

wait_for_ready() {
  local tries="${1:-120}"
  local delay_s="${2:-0.5}"

  for i in $(seq 1 "$tries"); do
    if curl -fsS "${BASE_URL}/api/files" >/dev/null; then
      return 0
    fi
    sleep "$delay_s"
  done

  echo "server did not become ready (${BASE_URL})" >&2
  return 1
}

wait_for_file_status() {
  local id="$1"
  local want="$2"
  local tries="${3:-600}"
  local delay_s="${4:-1.0}"

  for i in $(seq 1 "$tries"); do
    st=$(curl -fsS "${BASE_URL}/api/files" | python3 -c 'import json,sys; id=sys.argv[1]; items=json.load(sys.stdin); st=next((it.get("status","") for it in items if it.get("id")==id), ""); print(st)' "$id")
    if [ "$st" = "$want" ]; then
      return 0
    fi
    if [ "$st" = "failed" ]; then
      echo "upload processing failed" >&2
      return 1
    fi
    sleep "$delay_s"
  done

  echo "timeout waiting for status=${want}" >&2
  return 1
}

fetch_tile_and_encode() {
  local id="$1"
  local z="$2"
  local x="$3"
  local y="$4"

  local tile_data
  tile_data=$(curl -fsS "${BASE_URL}/api/files/${id}/tiles/${z}/${x}/${y}")

  # Encode to base64
  echo -n "$tile_data" | base64
}

generate_golden_for_dataset() {
  local dataset_name="$1"
  local fixture_path="$2"

  echo "Generating golden for $dataset_name..."
  echo "  Fixture: $fixture_path"

  # Upload fixture
  local upload_resp
  upload_resp=$(curl -fsS -F "file=@${fixture_path}" "${BASE_URL}/api/uploads")

  local id
  id=$(python3 -c 'import json,sys; print(json.loads(sys.stdin.read())["id"])' <<<"$upload_resp")

  echo "  Uploaded ID: $id"

  # Wait for ready
  wait_for_file_status "$id" ready
  echo "  File is ready"

  # Build JSON structure
  local json_file="${OUTPUT_DIR}/golden_${dataset_name}_tiles.json"

  cat > "$json_file" <<EOF
{
  "dataset": "$dataset_name",
  "fixture": "$fixture_path",
  "generated_at": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "crs": "EPSG:4326",
  "bbox": [-122.45, 37.76, -122.38, 37.80],
  "tiles": [
EOF

  local first=true
  for coord in "${TILE_COORDS[@]}"; do
    IFS=':' read -r z x_hit y_hit x_empty y_empty x_boundary y_boundary <<< "$coord"

    # Fetch hit tile
    local hit_b64
    hit_b64=$(fetch_tile_and_encode "$id" "$z" "$x_hit" "$y_hit")
    local hit_sha
    hit_sha=$(echo -n "$hit_b64" | base64 -d | sha256sum | cut -d' ' -f1)

    # Fetch empty tile
    local empty_b64
    empty_b64=$(fetch_tile_and_encode "$id" "$z" "$x_empty" "$y_empty")
    local empty_sha
    empty_sha=$(echo -n "$empty_b64" | base64 -d | sha256sum | cut -d' ' -f1)

    # Fetch boundary tile
    local boundary_b64
    boundary_b64=$(fetch_tile_and_encode "$id" "$z" "$x_boundary" "$y_boundary")
    local boundary_sha
    boundary_sha=$(echo -n "$boundary_b64" | base64 -d | sha256sum | cut -d' ' -f1)

    # Add comma if not first
    if [ "$first" = false ]; then
      echo "," >> "$json_file"
    fi
    first=false

    # Append tile entries
    cat >> "$json_file" <<EOF
    {
      "z": $z,
      "hit": {
        "x": $x_hit,
        "y": $y_hit,
        "sha256": "$hit_sha",
        "base64": "$hit_b64"
      },
      "empty": {
        "x": $x_empty,
        "y": $y_empty,
        "sha256": "$empty_sha",
        "base64": "$empty_b64"
      },
      "boundary": {
        "x": $x_boundary,
        "y": $y_boundary,
        "sha256": "$boundary_sha",
        "base64": "$boundary_b64"
      }
    }
EOF

    echo "  z=$z: hit($x_hit,$y_hit) empty($x_empty,$y_empty) boundary($x_boundary,$y_boundary)"
  done

  # Close JSON
  cat >> "$json_file" <<EOF

  ]
}
EOF

  echo "  Saved to $json_file"

  # Verify JSON is valid
  if ! python3 -m json.tool "$json_file" >/dev/null 2>&1; then
    echo "  ERROR: Invalid JSON generated!" >&2
    return 1
  fi

  echo "  ✓ Golden generated successfully"
}

main() {
  echo "Starting OSM tile golden generation..."
  echo "  Base URL: $BASE_URL"
  echo "  Output dir: $OUTPUT_DIR"
  echo ""

  # Wait for server
  wait_for_ready
  echo "Server is ready"
  echo ""

  # Create output dir
  mkdir -p "$OUTPUT_DIR"

  # Generate for each dataset
  for dataset in "${DATASETS[@]}"; do
    IFS=':' read -r name path <<< "$dataset"
    generate_golden_for_dataset "$name" "$path"
    echo ""
  done

  echo "All goldens generated successfully!"
  echo ""
  echo "Generated files:"
  ls -lh "${OUTPUT_DIR}"/golden_*.json
}

main "$@"
