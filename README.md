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
| Stable | `v*` tag push | Full release | `latest`, `vX.Y.Z` | Linux + macOS + Windows bundles |
| Nightly | Daily schedule (`02:00 UTC`) + manual dispatch | Pre-release | `nightly`, `nightly-YYYYMMDD`, `nightly-<sha>` | Linux + macOS + Windows bundles |

Each binary bundle contains:
- `mapflow` backend executable
- embedded DuckDB spatial extension (materialized to local cache/tmp on startup)
- `spatial-extension-manifest.json`

## Quickstart (Docker)

Prerequisites: Docker + Docker Compose v2.

Published images already contain `spatial.duckdb_extension`, so offline startup does not require runtime download.

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

1. Download an asset from [GitHub Releases](https://github.com/sharkAndshark/mapflow/releases):
   - Linux: `mapflow-*-linux-amd64.tar.gz`
   - macOS (Apple Silicon): `mapflow-*-darwin-arm64.tar.gz`
   - Windows: `mapflow-*-windows-amd64.zip`
2. Extract it, then run:

```bash
# Linux/macOS
./mapflow

# Windows (Command Prompt)
mapflow.exe

# Windows (PowerShell)
.\mapflow.exe
```

Binary bundles embed the spatial extension and auto-extract/load it on startup.
If the extracted extension in cache/tmp is cleaned up, MapFlow re-materializes it on next startup.
Frontend assets are embedded into the binary for bundle releases.

Optional runtime config:

```bash
# Linux/macOS
export WEB_DIST=./dist
export DB_PATH=./data/mapflow.duckdb
export UPLOAD_DIR=./uploads
export LISTEN=:3000
./mapflow

# Or use CLI flags
./mapflow --listen :8080 --listen-max-port 8100
```

```cmd
:: Windows (Command Prompt)
set WEB_DIST=.\dist
set DB_PATH=.\data\mapflow.duckdb
set UPLOAD_DIR=.\uploads
set LISTEN=:3000
mapflow.exe
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

| Env / CLI | Default | Description |
|---|---|---|
| `LISTEN` / `--listen` | `:3000` | Listen address (format: `[host]:port` or `:port`) |
| `LISTEN_MAX_PORT` / `--listen-max-port` | base+99 | Max port for fallback when port is in use |
| `DB_PATH` | `./data/mapflow.duckdb` | DuckDB path |
| `UPLOAD_DIR` | `./uploads` | Upload storage directory |
| `WEB_DIST` | `frontend/dist` | Optional external frontend assets path (if missing, bundle binaries use embedded assets) |
| `UPLOAD_MAX_SIZE_MB` | `200` | Upload max size |
| `COOKIE_SECURE` | `false` | Set `true` behind HTTPS |
| `CORS_ALLOWED_ORIGINS` | `http://localhost:3000` | Comma-separated CORS allowlist |
| `SPATIAL_EXTENSION_PATH` | unset | Explicit local spatial extension path |
| `SPATIAL_EXTENSION_DIR` | unset | Directory containing `spatial.duckdb_extension` |
| `SPATIAL_EXTENSION_CACHE_DIR` | unset | Preferred directory for extracted embedded spatial extension (set to a user-private directory for stricter permission requirements) |

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
just bump-duckdb 1.4.4
```

## Contracts & Internal Docs

- Behavior contracts: [docs/dev/behaviors.md](./docs/dev/behaviors.md)
- Internal architecture notes: [docs/internal.md](./docs/internal.md)
- Agent collaboration guidance: [AGENTS.md](./AGENTS.md)
