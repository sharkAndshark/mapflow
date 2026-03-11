# Internal

> 对该文档内容的增加必须极度克制，代码才是最好的文档。此文档只需要简明扼要提及技术栈，以及一些常见user story内部的技术流程。 

## 架构

```
React → HTTP → Axum → DuckDB
                    └─(tile_source=postgis)→ tokio-postgres → PostGIS

Windows 桌面集成:
  desktop(mapflow-desktop.exe, GUI 子系统) → 托盘 Exit → shutdown_signal → CHECKPOINT
  console(backend.exe, dev/CI) → Ctrl+C/关窗事件 → shutdown_signal → CHECKPOINT
```

**MBTiles 支持：**
- MBTiles 文件不导入 DuckDB，直接读取原始 SQLite 文件
- 通过 `tile_format` 字段区分动态（NULL）、MVT、PNG
- 矢量瓦片（MVT）：保留完整交互功能（特征点击、属性检查）
- 栅格瓦片（PNG）：仅静态显示，禁用交互

**DuckDB Spatial 扩展加载：**
- 默认开发构建不强制内嵌 extension（避免 fresh checkout 编译依赖本地二进制工件）
- release/self-contained 构建启用 `embed-spatial-extension`，并要求 `backend/extensions/spatial.duckdb_extension` 已准备好（dev: `just setup-dev`，CI: 自动下载）
- 启用嵌入时，启动时解包到本地 cache 目录后加载（支持离线部署）
- 解包使用原始文件名 `spatial.duckdb_extension`（DuckDB 根据文件名推导入口点），配合 `.checksum` 文件校验缓存
- cache 内容被清理后，启动时会自动重新解包；可通过 `SPATIAL_EXTENSION_CACHE_DIR` 指定更稳定/更严格权限的目录
- `backend/extensions/spatial-extension-manifest.json` 与 `Cargo.lock` 版本必须同步（CI 强校验）
- 无网络回退：移除了 `INSTALL spatial` 网络下载逻辑，确保完全离线运行

**PostGIS 数据源（MVP）：**
- 控制面仍在 DuckDB：`files` 增加 `tile_source`；新增 `postgis_connections` + `postgis_sources` 记录外部连接与源映射
- `/api/postgis/connections/test`：仅做连接探测与版本检查
- `/api/postgis/sources/register`：校验 schema/table(or view)/geom/fid 后注册为 `ready` 数据源（`type=postgis`）
- 运行时按 `tile_source` 分流：DuckDB 本地表 vs PostGIS 远端查询（私有 `/api/files/:id/tiles/...` 与公开 `/tiles/:slug/...`）
- 凭据存储：使用 `system_settings.app_secret` 派生密钥，对 PostGIS 密码做 AES-GCM 加密后写入 DuckDB
- 启动引导：`backend.exe` 与 `mapflow-desktop.exe` 都会先确保 `system_settings.app_secret` 存在（缺失时自动生成并持久化）

## 系统韧性

- **启动恢复**：先按 DuckDB 默认流程 replay WAL；若 open/replay 报 WAL 相关错误，则将 WAL 备份为 `*.wal.bak.<ts>` 后重试启动（`db::open_with_wal_recovery`）。可用 `WAL_RECOVERY_STRICT=1` 禁用自动隔离。
- **优雅关闭**：
  - Unix: SIGINT/SIGTERM 时执行 CHECKPOINT 刷入数据
  - Windows desktop: 托盘"退出"菜单触发优雅关闭（发布默认入口）
  - Windows console: Ctrl+C/控制台关闭事件触发优雅关闭尝试（开发/CI 入口）

## 认证

Session Cookie → axum-login → tower-sessions → DuckDB

## 技术栈

Axum 0.8, axum-login, tower-sessions, DuckDB, OpenLayers

## 发布基础设施

- Stable：`v*` tag 触发，发布 GHCR 多架构镜像与二进制 bundle 资产
- Nightly：每日 UTC 02:00 自动触发（也支持手动触发），发布 prerelease 与 nightly 镜像标签
- 二进制发布产物内嵌 `spatial.duckdb_extension`（按目标平台编译时注入），支持离线启动
- Windows 发布包仅暴露 desktop 入口（`mapflow-desktop.exe`），console 入口保留给开发/CI
