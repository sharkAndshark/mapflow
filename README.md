# MapFlow

[![CI](https://github.com/sharkAndshark/mapflow/actions/workflows/ci.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/ci.yml)
[![Release](https://github.com/sharkAndshark/mapflow/actions/workflows/release.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/release.yml)
[![Nightly](https://github.com/sharkAndshark/mapflow/actions/workflows/nightly.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/nightly.yml)
[![Security](https://github.com/sharkAndshark/mapflow/actions/workflows/security.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/security.yml)

MapFlow is a lightweight spatial data management app for data curators: upload files, inspect schema, preview tiles, and publish public tile URLs.

License: Apache-2.0

## Release Channels

| Channel | Trigger | GitHub Release | GHCR Tags | Assets |
|---|---|---|---|---|
| Stable | `v*` tag push | Full release | `latest`, `vX.Y.Z` | Linux + macOS bundles |
| Nightly | Daily schedule (`02:00 UTC`) + manual dispatch | Pre-release | `nightly`, `nightly-YYYYMMDD`, `nightly-<sha>` | Linux + macOS bundles |

Each binary bundle contains:
- `mapflow` backend executable
- prebuilt frontend (`dist/`)
- `spatial-extension-manifest.json`

## Quickstart (Docker)

Prerequisites: Docker + Docker Compose v2.

Run stable:

```bash
docker compose -f docker-compose.ghcr.yml up -d
```

Run nightly:

```bash
MAPFLOW_IMAGE=ghcr.io/sharkandshark/mapflow:nightly docker compose -f docker-compose.ghcr.yml up -d
```

Stop:

```bash
docker compose -f docker-compose.ghcr.yml down
```

## Quickstart (Binary Bundle)

1. Download an asset from [GitHub Releases](https://github.com/sharkAndshark/mapflow/releases).
2. Extract it, then run:

```bash
./mapflow
```

Optional runtime config:

```bash
export WEB_DIST=./dist
export DB_PATH=./data/mapflow.duckdb
export UPLOAD_DIR=./uploads
export PORT=3000
./mapflow
```

## Supported Upload Formats

- Shapefile (`.zip` with `.shp/.shx/.dbf`)
- GeoJSON (`.geojson`, `.json`)
- GeoJSONSeq / NDJSON (`.geojsonl`, `.geojsons`)
- KML (`.kml`)
- GPX (`.gpx`)
- TopoJSON (`.topojson`)
- MBTiles (`.mbtiles`, vector MVT + raster PNG)

## Runtime Configuration

| Env | Default | Description |
|---|---|---|
| `PORT` | `3000` | HTTP server port |
| `DB_PATH` | `./data/mapflow.duckdb` | DuckDB path |
| `UPLOAD_DIR` | `./uploads` | Upload storage directory |
| `WEB_DIST` | `frontend/dist` | Frontend static assets path |
| `UPLOAD_MAX_SIZE_MB` | `200` | Upload max size |
| `COOKIE_SECURE` | `false` | Set `true` behind HTTPS |
| `CORS_ALLOWED_ORIGINS` | `http://localhost:3000` | Comma-separated CORS allowlist |
| `SPATIAL_EXTENSION_PATH` | unset | Explicit local spatial extension path |
| `SPATIAL_EXTENSION_DIR` | unset | Directory containing `spatial.duckdb_extension` |

## Development

```bash
just install
just dev
```

Common commands:

```bash
just check
just test
just docker-up-build
```

## Contracts & Internal Docs

- Behavior contracts: [docs/dev/behaviors.md](./docs/dev/behaviors.md)
- Internal architecture notes: [docs/internal.md](./docs/internal.md)
- Agent collaboration guidance: [AGENTS.md](./AGENTS.md)
