# MapFlow

[![CI](https://github.com/sharkAndshark/mapflow/actions/workflows/ci.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/ci.yml)
[![Nightly](https://github.com/sharkAndshark/mapflow/actions/workflows/nightly.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/nightly.yml)
[![Security](https://github.com/sharkAndshark/mapflow/actions/workflows/security.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/security.yml)

> A lightweight spatial data management tool: upload, preview, and publish tile services

**Status**: Early version, rapid iteration (API may change)

## Core Features

- **Upload**: Supports Shapefile, GeoJSON, KML, GPX, TopoJSON, MBTiles, and more
- **Preview**: View spatial data and tiles online
- **Publish**: Generate public tile URLs

## Quick Start

```bash
docker run -d -p 3000:3000 ghcr.io/sharkandshark/mapflow:nightly
```

## Homebrew (Preview Channel)

MapFlow Homebrew distribution is currently an early preview channel.

- Breaking changes may be introduced at any time
- Upgrades are **not** guaranteed to be backward compatible
- Back up `~/.mapflow` before `brew upgrade`

```bash
brew tap sharkAndshark/mapflow
brew install sharkAndshark/mapflow/mapflow-preview
```

## Supported Formats

- Shapefile (`.zip` containing `.shp/.shx/.dbf`)
- GeoJSON (`.geojson`, `.json`)
- GeoJSONSeq / NDJSON (`.geojsonl`, `.geojsons`)
- KML (`.kml`)
- GPX (`.gpx`)
- TopoJSON (`.topojson`)
- MBTiles (`.mbtiles`, vector MVT + raster PNG)

## License

Apache-2.0

---

[中文文档](./README_zh.md)
