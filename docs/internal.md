# Internal

> 对该文档内容的增加必须极度克制，代码才是最好的文档。此文档只需要简明扼要提及技术栈，以及一些常见user story内部的技术流程。 

## 架构

React → HTTP → Axum → DuckDB

**MBTiles 支持：**
- MBTiles 文件不导入 DuckDB，直接读取原始 SQLite 文件
- 通过 `tile_format` 字段区分动态（NULL）、MVT、PNG
- 矢量瓦片（MVT）：保留完整交互功能（特征点击、属性检查）
- 栅格瓦片（PNG）：仅静态显示，禁用交互

## 认证

Session Cookie → axum-login → tower-sessions → DuckDB

## 技术栈

Axum 0.8, axum-login, tower-sessions, DuckDB, OpenLayers
