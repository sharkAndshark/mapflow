# 系统行为与测试契约

本文档定义 MapFlow 的可观测行为契约及其验证方法。

## 概览

**目标：** 提供安全的、基于认证的空间数据管理平台，允许管理员上传、列表和预览空间数据文件。

**访问控制：**
- 所有管理功能需要认证
- 支持首次设置和用户管理
- 基于角色的权限控制（admin/user）

**支持的格式：**
- **Shapefile：** 必须是包含 `.shp`、`.shx`、`.dbf` 的 `.zip` 压缩包
- **GeoJSON：** 标准的 `.geojson` 文件（单文件）

## 行为契约表

| ID | 模块 | 可观测行为 | 验证标准 | 验证命令 | 层级 | 优先级 |
|----|------|-----------|---------|---------|------|--------|
| API-001 | 上传 | POST /api/uploads 需要认证，接收文件，返回元数据 | 200 / 400 / 401 / 413 | `cargo test test_upload_*` | Integration | P0 |
| API-002 | 文件列表 | GET /api/files 需要认证，返回文件列表 | 200 / 401 | `cargo test test_files_list` | Integration | P0 |
| API-003 | 预览状态 | GET /api/files/:id/preview 需要认证，返回边界框 | 200 / 401 / 404 / 409 | `cargo test test_preview_ready` | Integration | P0 |
| API-004 | Tile 瓦片 | GET /api/files/:id/tiles/:z/:x/:y 需要认证，返回 MVT | 200 / 401 / 400 / 404 / 409 | `cargo test test_tiles_*` | Integration | P0 |
| API-005 | 特征属性 | GET /api/files/:id/features/:fid 需要认证，返回属性 | 200 / 401 / 404 / 409 | `cargo test test_features_*` | Integration | P0 |
| API-006 | Schema 查询 | GET /api/files/:id/schema 需要认证，返回字段列表 | 200 / 401 / 404 / 409 | `cargo test test_schema_*` | Integration | P1 |
| API-007 | 测试端点 | POST /api/test/reset 重置数据，仅测试模式 | 200 / 403 | `cargo test test_reset` | Integration | P2 |
| AUTH-001 | 首次设置 | POST /api/auth/init 创建初始管理员 | 200 / 400 / 409 | `cargo test test_init` | Integration | P0 |
| AUTH-002 | 登录 | POST /api/auth/login 验证凭证，设置会话 | 200 / 401 | `cargo test test_login` | Integration | P0 |
| AUTH-003 | 登出 | POST /api/auth/logout 清除会话 | 204 / 401 | `cargo test test_logout` | Integration | P0 |
| AUTH-004 | 检查状态 | GET /api/auth/check 返回当前用户 | 200 / 401 | `cargo test test_check` | Integration | P0 |
| STORE-001 | 文件存储 | 上传的文件存储在文件系统 | 文件存在 | `cargo test test_storage_*` | Integration | P0 |
| STORE-002 | 数据库 | 元数据存储在数据库，支持查询 | 数据可查询 | `pytest test_db_schema` | Unit | P0 |
| STORE-003 | 状态机 | 文件状态转换符合生命周期 | 状态转换合法 | `pytest test_state_machine` | Unit | P0 |
| UI-001 | 预览可用性 | 仅 ready 状态可打开预览 | 按钮状态正确 | `npm run test:e2e` | E2E | P0 |
| UI-002 | 特征检查器 | 显示属性字段，区分 NULL 和空字符串 | 显示正确 | `npm run test:e2e` | E2E | P0 |
| UI-003 | 字段信息 | 显示数据集字段名和类型 | 字段信息正确 | `npm run test:e2e` | E2E | P1 |
| UI-004 | 登录页面 | /login 显示登录表单，验证后跳转 | 跳转成功 | `npm run test:e2e` | E2E | P0 |
| UI-005 | 首次设置 | /init 显示管理员创建表单 | 表单可提交 | `npm run test:e2e` | E2E | P0 |
| UI-006 | 路由守卫 | 未认证访问受保护路由跳转登录页 | 自动跳转 | `npm run test:e2e` | E2E | P0 |
| E2E-001 | 完整上传 | 上传 → 处理 → 预览完整流程 | 流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-002 | 重启持久化 | 重启后数据仍可访问 | 数据存在 | `npm run test:e2e` | E2E | P0 |
| E2E-003 | 认证流程 | 首次访问 → 设置 → 登录 → 使用 → 登出 | 状态正确 | `npm run test:e2e` | E2E | P0 |
| CI-001 | 冒烟测试 | Docker 构建 → 上传 → 瓦片 | 瓦片匹配 | `scripts/ci/smoke_test.sh` | Integration | P0 |
| OSM-001 | 瓦片生成 | 数据集生成正确瓦片 | 计数匹配 | `cargo test test_tile_golden_*` | Integration | P1 |

## 参考

- 详细 API 规范见源码
- 架构说明见 `docs/internal.md`
- 协作原则见 `AGENTS.md`
