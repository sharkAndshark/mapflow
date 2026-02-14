# Internal

> 对该文档内容的增加必须极度克制，代码才是最好的文档。此文档只需要简明扼要提及技术栈，以及一些常见user story内部的技术流程。 

## 架构

React → HTTP → Axum → DuckDB

**MBTiles 支持：**
- MBTiles 文件不导入 DuckDB，直接读取原始 SQLite 文件
- 通过 `tile_format` 字段区分动态（NULL）、MVT、PNG
- 矢量瓦片（MVT）：保留完整交互功能（特征点击、属性检查）
- 栅格瓦片（PNG）：仅静态显示，禁用交互

**DuckDB Spatial 扩展加载：**
- 启动时优先加载本地 `spatial.duckdb_extension`（支持离线部署）
- 本地加载失败时回退到 DuckDB 默认 `LOAD/INSTALL spatial` 流程
- `backend/extensions/spatial-extension-manifest.json` 与 `Cargo.lock` 版本必须同步（CI 强校验）

## 认证

Session Cookie → axum-login → tower-sessions → DuckDB

## 技术栈

Axum 0.8, axum-login, tower-sessions, DuckDB, OpenLayers

## 发布基础设施

- Stable：`v*` tag 触发，发布 GHCR 多架构镜像与二进制 bundle 资产
- Nightly：每日 UTC 02:00 自动触发（也支持手动触发），发布 prerelease 与 nightly 镜像标签
- 发布产物内置 `spatial.duckdb_extension`（按目标平台打包），支持离线启动
