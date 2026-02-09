# 系统行为与测试契约

本文档定义 MapFlow 的可观测行为契约及其验证方法。

## 概览

**目标：** 允许数据管理员上传、列表和预览空间数据文件（探索者模式）。

**支持的格式：**
- **Shapefile：** 必须是包含 `.shp`、`.shx`、`.dbf` 的 `.zip` 压缩包
- **GeoJSON：** 标准的 `.geojson` 文件（单文件）

**测试覆盖的几何类型：**
- ✅ Point (OSM-002: sf_points)
- ✅ LineString (OSM-001: sf_lines)
- ✅ Polygon (OSM-004: sf_simple_polygons) 🆕
- ✅ MultiPoint (OSM-005: sf_multipoints) 🆕
- ✅ MultiLineString (OSM-006: sf_multilinestrings) 🆕
- ✅ MultiPolygon (OSM-003: sf_polygons)

> 💡 **启发性提示**：当本表格超过 30 行时，考虑：
> - 按模块分类（API/存储/UI）
> - 按层级分类（Unit/Integration/E2E）
> - 按优先级分类（P0/P1/P2）
> - 提取高频模式到独立表格

## 行为契约表

| ID | 模块 | 可观测行为 | 验证标准 | 验证命令 | 层级 | 优先级 |
|----|------|-----------|---------|---------|------|--------|
| API-001 | 上传 | POST /api/uploads 接收 multipart/form-data，最大大小 UPLOAD_MAX_SIZE_MB，返回文件元数据 JSON | 200 + 元数据 / 400（格式无效） / 413（超大小） + `{error}` | `cargo test test_upload_*` | Integration | P0 |
| API-002 | 文件列表 | GET /api/files 返回文件列表（id/name/type/size/uploadedAt/status/crs/path/error） | 200 + 列表 JSON | `cargo test test_files_list` | Integration | P0 |
| API-003 | 预览状态 | GET /api/files/:id/preview 仅在 ready 状态返回数据 | 200 + bbox(minx,miny,maxx,maxy,WGS84) / 404/409 + `{error}` | `cargo test test_preview_ready` | Integration | P0 |
| API-004 | Tile 瓦片 | GET /api/files/:id/tiles/:z/:x/:y 返回 MVT（Web Mercator 投影），包含几何和特征属性 | 200 + Content-Type=mvt / 400/404/409 + `{error}` | `cargo test test_tiles_*` | Integration | P0 |
| API-005 | 特征属性 | GET /api/files/:id/features/:fid 返回稳定 schema 的属性（NULL 值保留），按 ordinal 排序 | 200 + `{fid, properties:[{key,value}]}` / 404/409 + `{error}` | `cargo test test_features_*` | Integration | P0 |
| API-006 | Schema 查询 | GET /api/files/:id/schema 返回 `{fields:[{name,type}]}`，type 为 MVT 兼容类型（VARCHAR/INTEGER/BIGINT/DOUBLE/GEOMETRY），按 ordinal 排序，仅 ready 状态可访问 | 200 + `{fields}` / 404/409 + `{error}` | `cargo test test_schema_*` | Integration | P1 |
| API-007 | 测试端点 | POST /api/test/reset 重置数据库和存储，仅在 debug + MAPFLOW_TEST_MODE=1 | 执行成功，仅在 debug 构建 | `cargo test test_reset` | Integration | P2 |
| STORE-001 | 文件存储 | 原始文件存储在 `./uploads/<id>/`（由 UPLOAD_DIR 控制） | 文件存在且路径正确 | `cargo test test_storage_*` | Integration | P0 |
| STORE-002 | 数据库 Schema | DuckDB 表 files（元数据）、dataset_columns（列映射）、每个数据集的表（空间数据） | 表结构存在，数据可查询 | `pytest test_db_schema` | Unit | P0 |
| STORE-003 | 状态机 | 任务状态遵循 uploading → uploaded → processing → ready/failed 生命周期，processing 任务在重启时标记为 failed | 数据库状态转换合法，无非法转换 | `pytest test_state_machine` | Unit | P0 |
| UI-001 | 预览可用性 | UI 仅在 status=ready 时允许打开预览，非 ready 状态（uploaded/processing/failed）禁用 | 预览按钮状态正确 | `npm run test:e2e` | E2E | P0 |
| UI-002 | 特征检查器 | 显示基于数据集 schema 的稳定属性字段，NULL 值显示为 `--`（斜体、静音），空字符串显示为 `""`（悬停区分） | NULL 和空字符串正确区分 | `npm run test:e2e` | E2E | P0 |
| UI-003 | 字段信息显示 | Detail Sidebar 在 status=ready 时显示"字段信息"section，列出字段名和类型，支持加载中和错误状态 | 字段信息正确显示，状态转换正确 | `npm run test:e2e` | E2E | P1 |
| E2E-001 | 完整上传（GeoJSON） | 上传 .geojson → 列表更新 → ready → 详情可访问 → 预览打开地图 | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-002 | 完整上传（Shapefile） | 上传 .zip（.shp/.shx/.dbf）→ 列表更新 → ready → 详情可访问 → 预览打开地图 | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-003 | 重启持久化 | 重启后之前上传的文件仍可访问 | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| E2E-004 | 预览集成 | 点击预览 → 新标签页打开 → 地图加载 → 瓦片请求成功（200 OK 且非空） | 端到端流程成功 | `npm run test:e2e` | E2E | P0 |
| CI-001 | 冒烟测试 | 构建 Docker → 上传 GeoJSON → 等待 ready → 获取瓦片 | 与 testdata/smoke/expected_sample_z0_x0_y0.mvt.base64 比较字节 | `scripts/ci/smoke_test.sh` | Integration | P0 |
| OSM-001 | 瓦片生成（lines） | OSM sf_lines（20,898 道路特征）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_lines_samples` | Integration | P1 |
| OSM-002 | 瓦片生成（points） | OSM sf_points（交通信号灯、地点）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_points_samples` | Integration | P1 |
| OSM-003 | 瓦片生成（polygons） | OSM sf_polygons（31,715 建筑/土地利用特征，MultiPolygon几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_polygons_samples` | Integration | P1 |
| OSM-004 | 瓦片生成（simple polygons） | OSM sf_simple_polygons（10,000 简单多边形，Polygon几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_simple_polygons_samples` | Integration | P1 |
| OSM-005 | 瓦片生成（multipoints） | OSM sf_multipoints（402 多点要素，MultiPoint几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_multipoints_samples` | Integration | P1 |
| OSM-006 | 瓦片生成（multilinestrings） | OSM sf_multilinestrings（511 多线要素，MultiLineString几何）数据集生成正确瓦片（z=0,10,14 各 5 个样本） | 特征计数匹配 golden 配置 | `cargo test test_tile_golden_osm_multilinestrings_samples` | Integration | P1 |

## 快速决策指南

添加新测试时，问自己：

1. **这是什么类型的行为？**
   - 纯业务逻辑/数据转换 → Unit Test
   - HTTP API 契约/DB 状态 → Integration Test
   - 跨边界用户旅程 → E2E Test

2. **这个测试稳定且快速吗？**
   - 是 ✅ 继续使用
   - 否 → 考虑重构设计

3. **测试覆盖了稳定的契约还是实现细节？**
   - 稳定契约（API 响应、状态转换）✅
   - 实现细节（内部结构、时间字符串）→ 调整测试焦点

详细原则见 `AGENTS.md` 的"验证原则"部分。

## 数据模型参考

### Schema API 响应模型

```typescript
interface FileSchemaResponse {
  fields: FieldInfo[];
}

interface FieldInfo {
  name: string;  // 原始字段名（original_name）
  type: string;  // MVT 兼容类型（VARCHAR/INTEGER/BIGINT/DOUBLE/GEOMETRY）
}
```

**类型映射规则：**
- `VARCHAR`: 文本类型（包括空字符串）
- `INTEGER`: 32位整数
- `BIGINT`: 64位整数（包括从 SMALLINT/TINYINT 转换）
- `DOUBLE`: 浮点数（包括从 FLOAT 转换）
- `GEOMETRY`: 几何类型（通常为 `geom` 字段，在属性列表中排除）

**查询行为：**
- 仅对 `status=ready` 的文件返回 schema
- 字段按 `ordinal` 排序（导入时的字段顺序）
- 排除系统字段：`fid`（特征ID）、`geom`（几何）
- NULL 值在属性查询中保留（参见 API-005）

## 参考

- **DuckDB Spatial 函数**：`/Users/zhangyijun/RiderProjects/duckdb-spatial/docs`
