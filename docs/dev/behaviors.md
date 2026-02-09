# 系统行为与测试契约

本文档定义 MapFlow 的可观测行为契约及其验证方法。

## 概览

**目标：** 提供安全的、基于认证的空间数据管理平台，允许管理员上传、列表、预览和**公开发布**空间数据文件。

**访问控制：**
- 所有管理功能需要认证
- 支持首次设置和用户管理
- 基于角色的权限控制（admin/user）
- **公开瓦片服务**：发布后的文件可通过公共 URL 访问，无需认证

**支持的格式：**
- **Shapefile：** 必须是包含 `.shp`、`.shx`、`.dbf` 的 `.zip` 压缩包
- **GeoJSON：** 标准的 `.geojson` 文件（单文件）
- **GeoJSONSeq：** 按行分割的 GeoJSON（`.geojsonl`, `.geojsons`）
- **KML：** Keyhole Markup Language (`.kml`)
- **GPX：** GPS Exchange Format (`.gpx`)
- **TopoJSON：** 拓扑优化的 GeoJSON (`.topojson`)

**测试覆盖的几何类型：**
- ✅ Point (OSM-002: sf_points)
- ✅ LineString (OSM-001: sf_lines)
- ✅ Polygon (OSM-004: sf_simple_polygons) 🆕
- ✅ MultiPoint (OSM-005: sf_multipoints) 🆕
- ✅ MultiLineString (OSM-006: sf_multilinestrings) 🆕
- ✅ MultiPolygon (OSM-003: sf_polygons)

## 行为契约表

| ID | 模块 | 可观测行为 | 验证标准 | 验证命令 | 层级 | 优先级 |
|----|------|-----------|---------|---------|------|--------|
| API-001 | 上传 | POST /api/uploads 需要认证，接收 multipart/form-data，最大大小 UPLOAD_MAX_SIZE_MB，返回文件元数据 JSON | 200 + 元数据 / 400（格式无效） / 401（未认证） / 413（超大小） + `{error}` | `cargo test test_upload_*` | Integration | P0 |
| API-002 | 文件列表 | GET /api/files 需要认证，返回文件列表（id/name/type/size/uploadedAt/status/crs/path/error） | 200 + 列表 JSON / 401 | `cargo test test_files_list` | Integration | P0 |
| API-003 | 预览状态 | GET /api/files/:id/preview 需要认证，仅在 ready 状态返回数据 | 200 + bbox(minx,miny,maxx,maxy,WGS84) / 401 / 404 / 409 + `{error}` | `cargo test test_preview_ready` | Integration | P0 |
| API-004 | Tile 瓦片 | GET /api/files/:id/tiles/:z/:x/:y 需要认证，返回 MVT（Web Mercator 投影），包含几何和特征属性 | 200 / 401 / 400 / 404 / 409 | `cargo test test_tiles_*` | Integration | P0 |
| API-005 | 特征属性 | GET /api/files/:id/features/:fid 需要认证，返回稳定 schema 的属性（NULL 值保留），按 ordinal 排序 | 200 / 401 / 404 / 409 | `cargo test test_features_*` | Integration | P0 |
| API-006 | Schema 查询 | GET /api/files/:id/schema 需要认证，返回 `{fields:[{name,type}]}`，type 为 MVT 兼容类型，按 ordinal 排序，仅 ready 状态可访问 | 200 / 401 / 404 / 409 | `cargo test test_schema_*` | Integration | P1 |
| API-007 | 发布文件 | POST /api/files/:id/publish 需要认证，设置 `is_public=TRUE` 并分配 `public_slug`，可选自定义 slug（默认文件 ID），返回公开 URL 模板 | 200 + `{url,slug,isPublic}` / 400（slug 无效/冲突） / 401 / 404 / 409 | `cargo test test_publish_*` | Integration | P0 |
| API-008 | 取消发布 | POST /api/files/:id/unpublish 需要认证，设置 `is_public=FALSE` 并清空 `public_slug` | 200 / 401 / 404 | `cargo test test_unpublish_*` | Integration | P0 |
| API-009 | 公开地址 | GET /api/files/:id/public-url 需要认证，返回当前文件的公开 URL 模板 | 200 + `{slug,url}` / 401 / 404 | `cargo test test_public_url_*` | Integration | P1 |
| API-010 | 公开瓦片 | GET /tiles/:slug/:z/:x/:y **无需认证**，验证 `public_slug` 存在且 `is_public=TRUE`，返回 MVT 瓦片 | 200 + MVT / 400 / 404 | `cargo test test_public_tiles_*` | Integration | P0 |
| API-011 | 测试端点 | POST /api/test/reset 重置数据库和存储，仅在 debug + MAPFLOW_TEST_MODE=1 | 执行成功，仅在 debug 构建 | `cargo test test_reset` | Integration | P2 |
| AUTH-001 | 首次设置 | POST /api/auth/init 创建初始管理员 | 200 / 400 / 409 / 500 | `npm run test:e2e` | E2E | P0 |
| AUTH-002 | 登录 | POST /api/auth/login 验证凭证，设置会话 | 200 / 401 / 500 | `npm run test:e2e` | E2E | P0 |
| AUTH-003 | 登出 | POST /api/auth/logout 清除会话 | 204 / 500 | `npm run test:e2e` | E2E | P0 |
| AUTH-004 | 检查状态 | GET /api/auth/check 返回当前用户 | 200 / 401 | `npm run test:e2e` | E2E | P0 |
| STORE-001 | 文件存储 | 原始文件存储在 `./uploads/<id>/`（由 UPLOAD_DIR 控制） | 文件存在且路径正确 | `cargo test test_storage_*` | Integration | P0 |
| STORE-002 | 数据库 Schema | DuckDB 表 files（元数据）、dataset_columns（列映射）、每个数据集的表（空间数据） | 表结构存在，数据可查询 | `pytest test_db_schema` | Unit | P0 |
| STORE-003 | 状态机 | 任务状态遵循 uploading → uploaded → processing → ready/failed 生命周期，processing 任务在重启时标记为 failed | 数据库状态转换合法，无非法转换 | `pytest test_state_machine` | Unit | P0 |
| UI-001 | 预览可用性 | UI 仅在 status=ready 时允许打开预览，非 ready 状态（uploaded/processing/failed）禁用 | 预览按钮状态正确 | `npm run test:e2e` | E2E | P0 |
| UI-002 | 特征检查器 | 显示基于数据集 schema 的稳定属性字段，NULL 值显示为 `--`（斜体、静音），空字符串显示为 `""`（悬停区分） | NULL 和空字符串正确区分 | `npm run test:e2e` | E2E | P0 |
| UI-003 | 字段信息显示 | Detail Sidebar 在 status=ready 时显示"字段信息"section，列出字段名和类型，支持加载中和错误状态 | 字段信息正确显示，状态转换正确 | `npm run test:e2e` | E2E | P1 |
| UI-004 | 登录页面 | /login 显示登录表单，验证后跳转 | 跳转成功 | `npm run test:e2e` | E2E | P0 |
| UI-005 | 首次设置 | /init 显示管理员创建表单 | 表单可提交 | `npm run test:e2e` | E2E | P0 |
| UI-006 | 路由守卫 | 未认证访问受保护路由跳转登录页 | 自动跳转 | `npm run test:e2e` | E2E | P0 |
| UI-007 | 发布按钮 | 文件列表每行显示"发布/复制/取消发布"操作按钮（仅 ready 状态），已发布文件显示"复制"和"取消发布" | 按钮状态正确 | `npm run test:e2e` | E2E | P0 |
| UI-008 | 发布弹窗 | 点击"发布"打开模态框，显示文件名、slug 输入框（默认文件 ID）、公开地址预览，提交后更新列表 | 弹窗交互正确 | `npm run test:e2e` | E2E | P0 |
| E2E-001 | 完整上传（GeoJSON） | 上传 .geojson → 列表更新 → ready → 详情可访问 → 预览打开地图 | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-002 | 完整上传（Shapefile） | 上传 .zip（.shp/.shx/.dbf）→ 列表更新 → ready → 详情可访问 → 预览打开地图 | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-003 | 完整上传（GeoJSONSeq） | 上传 .geojsonl → 列表更新 → ready → schema 查询 → 瓦片端点验证成功 | 端到端流程成功 | `cargo test test_upload_geojsonseq_lifecycle` | Integration | P0 |
| E2E-004 | 完整上传（KML） | 上传 .kml → 列表更新 → ready → schema 查询 → 瓦片端点验证成功 | 端到端流程成功 | `cargo test test_upload_kml_lifecycle` | Integration | P0 |
| E2E-005 | 完整上传（GPX） | 上传 .gpx → 列表更新 → ready → schema 查询 → 瓦片端点验证成功 | 端到端流程成功 | `cargo test test_upload_gpx_lifecycle` | Integration | P0 |
| E2E-006 | 完整上传（TopoJSON） | 上传 .topojson → 列表更新 → ready → schema 查询 → 瓦片端点验证成功 | 端到端流程成功 | `cargo test test_upload_topojson_lifecycle` | Integration | P0 |
| E2E-007 | 重启持久化 | 重启后之前上传的文件仍可访问 | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-008 | 预览集成 | 点击预览 → 新标签页打开 → 地图加载 → 瓦片请求成功（200 OK 且非空） | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-009 | 认证流程 | 首次访问 → 设置 → 登录 → 使用 → 登出 | 状态正确 | `npm run test:e2e` | E2E | P0 |
| E2E-010 | 发布流程 | 上传文件 → ready → 点击发布 → 自定义 slug → 确认 → 复制公开地址 → 无需认证访问瓦片 | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| CI-001 | 冒烟测试 | 构建 Docker → 上传 GeoJSON → 等待 ready → 获取瓦片 | 与 testdata/smoke/expected_sample_z0_x0_y0.mvt.base64 比较字节 | `scripts/ci/smoke_test.sh` | Integration | P0 |
| OSM-001 | 瓦片生成（lines） | OSM sf_lines（20,898 道路特征）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_lines_samples` | Integration | P1 |
| OSM-002 | 瓦片生成（points） | OSM sf_points（交通信号灯、地点）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_points_samples` | Integration | P1 |
| OSM-003 | 瓦片生成（polygons） | OSM sf_polygons（31,715 建筑/土地利用特征，MultiPolygon几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_polygons_samples` | Integration | P1 |
| OSM-004 | 瓦片生成（simple polygons） | OSM sf_simple_polygons（10,000 简单多边形，Polygon几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_simple_polygons_samples` | Integration | P1 |
| OSM-005 | 瓦片生成（multipoints） | OSM sf_multipoints（402 多点要素，MultiPoint几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_multipoints_samples` | Integration | P1 |
| OSM-006 | 瓦片生成（multilinestrings） | OSM sf_multilinestrings（511 多线要素，MultiLineString几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_multilinestrings_samples` | Integration | P1 |

## 参考

- 详细 API 规范见源码
- 架构说明见 `docs/internal.md`
- 协作原则见 `AGENTS.md`
