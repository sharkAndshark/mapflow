# Known Issues & TODOs

**Last Updated**: 2026-02-23

## Known Issues

### Flaky CI Tests

#### Spatial Import GDAL Error (No such file)

- **Tests**:
  - `test_feature_properties_endpoint_returns_null_for_missing_values`
  - `test_public_tile_respects_minzoom_only`
- **Symptom**: `Spatial import failed: IO Error: GDAL Error (4): /tmp/.../points.geojson: No such file or directory`
- **Evidence**:
  - CI Runs: 22262024670, 22289556132, Ubuntu 24.04
  - 失败频率: ~3% (30+ CI 运行中偶发)
  - 本地运行全部通过
  - 同一 commit 在 push 事件通过，PR 事件失败（flaky 特征）
- **Root Cause**: 高并发 I/O 压力下 GDAL/DuckDB ST_Read 偶发竞态条件。虽然 `sync_all()` 已调用，CI 虚拟化环境文件系统同步可能存在延迟
- **Mitigation**: `--test-threads=2` 限制并发 (已应用)
- **Priority**: Low

#### SIGSEGV

- **Symptom**: `backend_tests` 偶发 `SIGSEGV` (signal 11) 进程崩溃
- **Evidence**:
  - 本地测试从未复现
  - CI 并行运行中一个成功一个失败（flaky）
  - 崩溃发生在 40+ 测试之后，不是特定测试
- **Root Cause**: DuckDB `bundled` feature 在 CI Linux 环境中的内存管理边缘情况
- **Workaround**: 可考虑 `cargo test -- --test-threads=2` 限制并发
- **Priority**: Low（非代码问题，等待 DuckDB 新版本）

## Performance Improvements

### CI Docker Smoke Test

- **Status**: ✅ Done (2026-02-16)
- **Issue**: docker_smoke in CI took ~15min with poor ROI (e2e already covers functionality)
- **Solution**: Removed from CI, kept only in release/nightly workflows for Docker image validation

## Custom CRS Support (Phase 1)

**Goal**: 支持预览自定义坐标系数据（如本地测量坐标、建筑平面图）

**Scope**: 单文件预览，不支持发布公开瓦片

### User Story

1. 用户上传本地坐标系数据（无 EPSG code）
2. 系统自动识别为 custom CRS
3. Preview 正常显示，坐标轴显示原始坐标
4. 用户可手动修改 CRS 定义

### CRS Classification Rules

```
ST_Read_Meta 结果 → 归一化 + 分类
├── NULL（无 CRS 声明）→ crs=null, crs_type=custom
├── EPSG:XXXX → crs="EPSG:XXXX", crs_type=standard
├── WGS84 / CRS84 → crs="EPSG:4326", crs_type=standard
├── WKT + AUTHORITY["EPSG","XXXX"] → crs="EPSG:XXXX", crs_type=standard
├── WKT 无 EPSG AUTHORITY → crs=完整WKT, crs_type=custom
└── 其他字符串 → crs=原值, crs_type=custom
```

### Tasks

#### 1. Database
- [x] files 表添加 `crs_type VARCHAR DEFAULT 'standard'`
- [x] files 表添加 `data_bounds VARCHAR` (JSON: {minx, miny, maxx, maxy})

#### 2. Backend

**2.1 新建 crs.rs**
- [x] `normalize_crs(raw: Option<&str>) -> NormalizedCrs`
- [x] `parse_wkt(wkt: &str) -> NormalizedCrs` - 解析 WKT 中的 AUTHORITY
- [x] `DataBounds` struct + `is_valid()` 方法
- [x] `calculate_custom_tile_bbox(bounds, z, x, y) -> (minx, miny, maxx, maxy)`

**2.2 models.rs**
- [x] FileItem 新增 `crs_type: Option<String>`
- [x] PreviewMeta 新增 `crs_type: String, data_bounds: Option<[f64; 4]>`
- [x] UpdateCrsRequest struct

**2.3 import.rs**
- [x] 调用 `normalize_crs()` 归一化 CRS
- [x] 更新 files.crs, files.crs_type
- [x] 计算所有文件的 data_bounds: `ST_Extent(geom)`

**2.4 tiles.rs**
- [x] TileParams struct
- [x] TileError struct + From<duckdb::Error>
- [x] `build_mvt_select_sql`: custom 用 `calculate_custom_tile_bbox` + `ST_MakeEnvelope`
- [x] `build_mvt_query_params`: 根据类型返回正确的参数

**2.5 handlers.rs**
- [x] `get_preview_meta`: 返回 crs_type, data_bounds
- [x] `get_preview_meta`: bbox 计算 - custom 用 data_bounds，standard 用 ST_Transform
- [x] `update_crs`: `PUT /api/files/:id/crs`
  - 请求体 `{ crs: string }` (必填)
  - 只更新元数据，不重算 data_bounds

**2.6 routes.rs**
- [x] 添加 PUT 方法到 CORS 配置
- [x] 添加 `/api/files/{id}/crs` 路由

**2.7 public.rs**
- [x] 更新 `get_public_tile` 支持自定义 CRS

#### 3. Frontend

**3.1 Preview.jsx**
- [x] 检测 `meta.crsType === 'custom'`
- [x] `calculateCustomResolutions()` 函数 + 边界检查
- [x] custom 时构建 TileGrid:
  - extent = meta.dataBounds
  - origin = [minx, maxy] (左上角)
  - resolutions = 计算 z=0 到 z=20
- [x] 创建 Projection: code, units='m', extent
- [x] 显示 custom CRS 标签（橙色样式）

#### 4. Test Data

**4.1 GeoJSON (testdata/custom-crs/)**
- [x] simple_custom_crs.geojson - 无 CRS → custom
- [x] complex_custom_crs.geojson - 自定义名 → custom
- [x] no_crs_test.geojson - 无 CRS → custom
- [x] negative_coords_test.geojson - 负坐标测试
- [x] README.md 更新

#### 5. Tests
- [x] Unit: crs.rs 各类输入 (9 tests)
- [x] Unit: calculate_custom_tile_bbox (2 tests)
- [x] Unit: DataBounds::is_valid (1 test)
- [x] Unit: DataBounds::is_valid_wgs84 (5 tests)
- [x] Backend: 67 tests passed
- [x] E2E: 自定义 CRS 上传流程 (custom-crs.spec.js)
- [x] E2E: 自定义 CRS 瓦片请求
- [x] E2E: 预览页正常显示
- [x] Integration: PUT /api/files/:id/crs (6 tests)

#### 6. Future Test Data (Optional)

**6.1 Shapefile (testdata/custom-crs/shapefile/)**
- [ ] custom_prj_no_auth.zip - .prj 无 EPSG AUTHORITY → custom
- [ ] custom_prj_with_auth.zip - .prj 有 EPSG AUTHORITY → standard
- [ ] no_prj_file.zip - 无 .prj 文件 → custom

**6.2 GeoJSONSeq**
- [ ] osm_lines_custom.geojsonl - 无 CRS → custom

**6.3 TopoJSON**
- [ ] osm_polygons_custom.topojson - 无 CRS → custom

### Key Implementation Details

**Custom Tile BBox Calculation:**
```rust
fn calculate_custom_tile_bbox(bounds: &DataBounds, z: i32, x: i32, y: i32) -> (f64, f64, f64, f64) {
    let tiles_per_side = 2f64.powi(z);
    let tile_width = (bounds.maxx - bounds.minx) / tiles_per_side;
    let tile_height = (bounds.maxy - bounds.miny) / tiles_per_side;
    
    let minx = bounds.minx + x as f64 * tile_width;
    let maxx = bounds.minx + (x + 1) as f64 * tile_width;
    let maxy = bounds.maxy - y as f64 * tile_height;  // Y 轴从上往下
    let miny = bounds.maxy - (y + 1) as f64 * tile_height;
    
    (minx, miny, maxx, maxy)
}
```

**Frontend Resolutions:**
```javascript
function calculateResolutions(bounds, maxZoom = 20) {
  const maxDim = Math.max(bounds.maxx - bounds.minx, bounds.maxy - bounds.miny);
  return Array.from({ length: maxZoom + 1 }, (_, z) => maxDim / (256 * Math.pow(2, z)));
}
```

---

## Code Quality Improvements

### CRS Validation in Tile Generation

- **Issue**: `tiles.rs` 中的 `source_crs` 在 standard 模式下直接插入 SQL
- **Current Protection**: `crs_type` 由 `normalize_crs` 控制，只有 EPSG 格式才会被标记为 standard
- **Risk**: 低（需要绕过 API 直接修改数据库才能利用）
- **Solution**: 在 `build_mvt_select_sql` 中添加 CRS 格式验证
- **Priority**: Low

### Debug Logging Cleanup

- **Status**: ✅ Done (2026-02-20)
- **Solution**: 已使用 tracing 框架，无 println!/eprintln! 残留

### MBTiles Connection Pool

- **Status**: ✅ Done (2026-02-16)
- **Solution**: moka cache + spawn_blocking
  - LRU cache with max 100 connections
  - spawn_blocking to avoid blocking tokio runtime

### Slug Race Condition

- **Current**: Manual uniqueness check before INSERT
- **Issue**: Small probability of race condition
- **Solution Options**:
  1. Database unique constraint (requires DuckDB support)
  2. Transaction with retry logic
- **Priority**: Low (acceptable for current phase)

## Code Review Findings (2026-02-15)

### behaviors.md 与实际不一致 (P0) ✅ Done (2026-02-16)

**解决方案**：
- 删除 API-011 契约（测试端点非用户行为）
- API-002/003：改为描述性验证方式（已有测试覆盖）
- 补充 OSM-004~006 测试函数（配置已就绪）
- 补充 KML lifecycle 测试（代表性复杂格式）
- E2E-003：指向现有前端 E2E (`upload-formats.spec.js`)
- E2E-005/006 (GPX/TopoJSON)：降为 P2，信任 GDAL 解析层

### API 契约状态码不一致 (P1) ✅ Done (2026-02-20)

**验证结果**：文档与实现已一致，无需修改
- API-001: 文档 201 / 实现 `StatusCode::CREATED` ✅
- API-004/010 空瓦片: 文档 204 / 实现 `StatusCode::NO_CONTENT` ✅

### 测试设计改进 (P1)

前端 E2E 存在固定等待：

| 文件 | 行号 | 当前实现 | 建议改为 |
|------|------|---------|---------|
| `preview.spec.js` | 76 | `waitForTimeout(2000)` | `waitForResponse()` |
| `zoom-limit.spec.js` | 86, 174 | `waitForTimeout(2000)` | `expect.poll()` |
| `publish.spec.js` | 55 | `waitForTimeout(1000)` | `waitForResponse()` |

### 缺失的 E2E 测试覆盖 (P2)

| 契约 | 缺失验证 |
|------|---------|
| UI-001 | 非 ready 状态预览按钮禁用的显式测试 |
| UI-003 | 特征高亮样式 (rgba(255,200,0,0.7)) 的 E2E 验证 |
| UI-010 | 前端缩放限制实际行为（非仅 API 返回值） |

### CI/CD 改进 (P2)

1. **DRY**: `nightly.yml` 与 `release.yml` 约 70% 重复代码 → 考虑提取 reusable workflow
2. **供应链安全**: DuckDB extension 下载缺少 SHA256 校验
3. **冒烟测试健壮性**: 缺少重试机制，网络抖动可能导致误报

### 轻微问题 (P3)

- UI-002: 空字符串显示样式与文档描述不完全一致（应增加视觉区分）
- PMTiles: 无 Range 请求时完整下载可能内存压力，考虑流式响应
- 前端 CSS: `.preview-page`, `.spinner`, `.badge` 等样式未定义
