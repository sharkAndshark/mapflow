# MapFlow

[![CI](https://github.com/sharkAndshark/mapflow/actions/workflows/ci.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/ci.yml)
[![Release](https://github.com/sharkAndshark/mapflow/actions/workflows/release.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/release.yml)
[![Nightly](https://github.com/sharkAndshark/mapflow/actions/workflows/nightly.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/nightly.yml)
[![Security](https://github.com/sharkAndshark/mapflow/actions/workflows/security.yml/badge.svg)](https://github.com/sharkAndshark/mapflow/actions/workflows/security.yml)

> 轻量级空间数据管理工具：上传、预览、发布瓦片服务

**状态**：早期版本，快速迭代中（API 可能变动）

## 核心功能

- **上传**：支持 Shapefile、GeoJSON、KML、GPX、TopoJSON、MBTiles 等格式
- **预览**：在线查看空间数据和瓦片
- **发布**：生成公开的瓦片 URL

## 一分钟上手

```bash
docker run -d -p 3000:3000 ghcr.io/sharkandshark/mapflow:nightly
```

## 支持格式

- Shapefile（`.zip` 包含 `.shp/.shx/.dbf`）
- GeoJSON（`.geojson`, `.json`）
- GeoJSONSeq / NDJSON（`.geojsonl`, `.geojsons`）
- KML（`.kml`）
- GPX（`.gpx`）
- TopoJSON（`.topojson`）
- MBTiles（`.mbtiles`，支持矢量 MVT 和栅格 PNG）

## License

Apache-2.0

---

[English](./README.md)
