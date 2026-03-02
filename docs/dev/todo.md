# Known Issues & TODOs

**Last Updated**: 2026-03-02

## Agent 接管执行清单（2026-03-02）

> 执行规则：严格逐条推进。每完成一条必须先运行该条对应验证命令并记录结果，通过后再进入下一条。

### 执行中（按顺序）

- [x] S1: 修复 PMTiles 公开端点在相对 `UPLOAD_DIR` 场景下的路径解析问题（`/tiles/:slug` + `HEAD /tiles/:slug`）
  - 验证：新增回归测试（相对 `UPLOAD_DIR` 场景）+ `cargo test test_pmtiles_*`
  - 结果（2026-03-02）：✅ 通过
    - `cargo test --manifest-path backend/Cargo.toml test_pmtiles_range_request -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml test_pmtiles_upload_and_publish -- --nocapture`
- [x] S2: 修复公开 MBTiles(MVT) 响应缺少 `Content-Encoding: gzip` 的不一致问题
  - 验证：新增/更新公开端点响应头断言 + `cargo test test_mbtiles_publish_and_public_tiles`
  - 结果（2026-03-02）：✅ 通过
    - `cargo test --manifest-path backend/Cargo.toml test_mbtiles_publish_and_public_tiles -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml test_public_mbtiles_png_returns_correct_content_type -- --nocapture`
- [x] S3: 修复 `GET /api/files/:id/public-url` 对 PMTiles 返回错误 URL 模板的问题
  - 验证：新增 PMTiles public-url 断言 + `cargo test test_public_url_* test_pmtiles_*`
  - 结果（2026-03-02）：✅ 通过
    - `cargo test --manifest-path backend/Cargo.toml test_public_url_endpoint -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml test_pmtiles_public_url_endpoint -- --nocapture`
- [x] S4: 前端上传入口补全 `.pmtiles`（文件选择器 accept + 文案）
  - 验证：前端单测/集成断言 + `npm --prefix frontend run test:unit`
  - 结果（2026-03-02）：✅ 通过
    - `npm --prefix frontend run test:unit`
- [x] S5: 将 `docs/dev/behaviors.md` 的验证引用收敛为精确测试函数/文件（避免宽泛命令）
  - 验证：抽样 spot-check + 文档自检
  - 结果（2026-03-02）：✅ 通过
    - 自检命令：基于 `cargo test -- --list` 对 `behaviors.md` 中 `test_*` 引用逐条匹配，结果 `OK`
- [x] S6: 新增 `behaviors` 引用 lint（校验文档中测试/文件引用真实存在）并接入 CI
  - 验证：`scripts/ci/lint_behaviors_refs.sh` 本地运行 + CI workflow 增加 step
  - 结果（2026-03-02）：✅ 通过
    - `bash scripts/ci/lint_behaviors_refs.sh` -> `behaviors references lint passed`
    - `ci.yml` 已增加 `Lint behaviors references` step
- [x] S7: 发布链路增加 spatial extension 下载完整性校验（SHA256）
  - 验证：脚本单测/集成验证 + release/nightly 关键路径 dry-run
  - 结果（2026-03-02）：✅ 通过（本地离线模拟）
    - 新增 `archive_sha256` manifest 字段，并在下载脚本中强校验
    - 通过 `file://` 本地归档模拟下载校验链路：`LOCAL_DOWNLOAD_VERIFY_OK`
    - 备注：当前环境无法解析 `extensions.duckdb.org`，因此未执行远端在线下载验证
- [x] S8: CRS transform SQL 输入收敛（standard 模式仅允许归一化 EPSG）
  - 验证：新增单测 + 瓦片回归测试
  - 结果（2026-03-02）：✅ 通过
    - `cargo test --manifest-path backend/Cargo.toml test_build_mvt_select_sql_normalizes_standard_crs -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml test_build_mvt_select_sql_rejects_invalid_standard_crs -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml test_public_tiles_endpoint -- --nocapture`
- [x] S9: Spatial 导入链路增加 ST_Read 瞬时文件错误重试（降低 flaky）
  - 验证：判定函数单测 + 真实接口回归
  - 结果（2026-03-02）：✅ 通过
    - `cargo test --manifest-path backend/Cargo.toml test_retryable_st_read_error_true_for_missing_file -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml test_retryable_st_read_error_false_for_sql_error -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml test_feature_properties_endpoint_returns_null_for_missing_values -- --nocapture`
- [x] S10: main.rs 端口绑定测试在受限环境显式 skip（避免误报）
  - 验证：端口测试过滤 + main.rs 全量单测
  - 结果（2026-03-02）：✅ 通过
    - `cargo test --manifest-path backend/Cargo.toml test_port_ -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml --bin backend -- --nocapture`
- [x] S11: Smoke 脚本关键 HTTP 调用增加可配置重试
  - 验证：脚本语法检查 + 重试函数失败分支验证 + behaviors lint
  - 结果（2026-03-02）：✅ 通过
    - `bash -n scripts/smoke/lib/common.sh scripts/smoke/smoke-binary.sh scripts/smoke/smoke-docker.sh`
    - `bash -lc 'source scripts/smoke/lib/common.sh; ... curl_with_retry ...'` 输出 `CURL_RETRY_FAIL_OK`
    - `bash scripts/ci/lint_behaviors_refs.sh` -> `behaviors references lint passed`
- [x] S12: CI backend_tests 稳定性增强（allocator 参数 + 分步执行 + 失败诊断重跑）
  - 验证：配置检查 + 后端回归跑通
  - 结果（2026-03-02）：✅ 通过
    - `.github/workflows/ci.yml` 已生效（`lib/api` 分步 + failure rerun）
    - `cargo test --manifest-path backend/Cargo.toml --lib --test api_tests -- --test-threads=1` 全通过（57 + 114）

## Known Issues

### Port-binding tests in restricted sandbox

- **Status**: ✅ Done (2026-03-02)
- **Issue**: `cargo test` 全量中 `src/main.rs` 的端口绑定测试在部分受限环境会报 `Operation not permitted`
- **Solution**:
  - `bind_with_fallback` 在整段端口范围都因权限不足失败时返回明确 permission error
  - 端口相关单测在识别到 permission-denied 场景时显式跳过（记录 `Skipping ... sandbox restrictions`），避免误报为代码回归
- **Verification**:
  - `cargo test --manifest-path backend/Cargo.toml test_port_ -- --nocapture`
  - `cargo test --manifest-path backend/Cargo.toml --bin backend -- --nocapture`

### Flaky CI Tests

#### Spatial Import GDAL Error (No such file)

- **Test**: `test_feature_properties_endpoint_returns_null_for_missing_values`
- **Symptom**: `Spatial import failed: IO Error: GDAL Error (4): /tmp/.../roads.geojson: No such file or directory`
- **Evidence**:
  - CI Run: 22262024670, Ubuntu 24.04
  - 失败频率: 30 次 CI 运行中仅 1 次 (~3.3%)
  - 本地运行 5 次全部通过
- **Root Cause**: 高并发 I/O 压力下 GDAL/DuckDB ST_Read 偶发竞态条件
- **Mitigation**:
  - `--test-threads=2` 限制并发 (已应用)
  - `import_spatial_data` 在 `ST_Read` 建表阶段新增重试（最多 3 次，50/100ms 退避，仅对 `GDAL Error (4) / No such file or directory` 类错误触发）（2026-03-02）
- **Priority**: Low

#### SIGSEGV

- **Symptom**: `backend_tests` 偶发 `SIGSEGV` (signal 11) 进程崩溃
- **Evidence**:
  - 本地测试从未复现
  - CI 并行运行中一个成功一个失败（flaky）
  - 崩溃发生在 40+ 测试之后，不是特定测试
- **Root Cause**: DuckDB `bundled` feature 在 CI Linux 环境中的内存管理边缘情况
- **Workaround**:
  - CI 中 `backend_tests` 已固定 `--test-threads=1`
  - CI `backend_tests` 增加 `MALLOC_ARENA_MAX=2`（降低 allocator 抖动）与 `RUST_BACKTRACE=1`（提升诊断信息）（2026-03-02）
  - CI `backend_tests` 拆分为 `lib` 与 `api_tests` 两步；失败时自动重跑 `api_tests --nocapture` 收集诊断日志（2026-03-02）
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

- **Status**: ✅ Done (2026-03-02)
- **Issue**: `tiles.rs` 中的 `source_crs` 在 standard 模式下直接插入 SQL
- **Solution**:
  - 在 `build_mvt_select_sql` 开始阶段新增 `validated_transform_source_crs` 校验
  - standard 模式仅接受可归一化为 `EPSG:<digits>` 的 CRS，并使用归一化结果写入 SQL
  - 非法输入直接返回 `TileError`，不进入 SQL 拼接
- **Verification**:
  - `cargo test --manifest-path backend/Cargo.toml test_build_mvt_select_sql_normalizes_standard_crs -- --nocapture`
  - `cargo test --manifest-path backend/Cargo.toml test_build_mvt_select_sql_rejects_invalid_standard_crs -- --nocapture`
  - `cargo test --manifest-path backend/Cargo.toml test_tile_invalid_coords_returns_400 -- --nocapture`
  - `cargo test --manifest-path backend/Cargo.toml test_public_tiles_endpoint -- --nocapture`

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

前端 E2E 固定等待治理（2026-03-02）：

| 文件 | 行号 | 当前实现 | 建议改为 |
|------|------|---------|---------|
| `preview.spec.js` | - | ✅ 已移除固定等待 | 使用 `expect.poll()` + 可观测条件 |
| `zoom-limit.spec.js` | - | ✅ 已移除固定等待 | 使用 `expect.poll()` + API 状态轮询 |
| `publish.spec.js` | - | ✅ 已移除固定等待 | 使用可观测 UI/接口状态断言 |

### 缺失的 E2E 测试覆盖 (P2)

| 契约 | 缺失验证 |
|------|---------|
| UI-001 | ✅ 已补充（2026-03-02）：`frontend/tests/polling.spec.js` 新增 `preview action is hidden before ready and shown after ready` |
| UI-003 | 特征高亮样式 (rgba(255,200,0,0.7)) 的 E2E 验证 |
| UI-010 | ✅ 已补充（2026-03-02）：`frontend/tests/zoom-limit.spec.js` 新增基于真实缩放交互 + tile z 观测的约束验证 |

### CI/CD 改进 (P2)

1. **CI 触发降噪**: ✅ 已完成（2026-03-02）  
   `ci.yml` 调整为 `push` 仅 `main` 触发，开发分支通过 `pull_request` 触发；并增加 workflow `concurrency` 自动取消旧 run，减少 PR 期间重复计算
2. **供应链安全**: DuckDB extension 下载缺少 SHA256 校验
3. **冒烟测试健壮性**: ✅ 已完成（2026-03-02）  
   `scripts/smoke/lib/common.sh` 已为关键 HTTP 调用增加可配置重试（`SMOKE_HTTP_RETRIES` / `SMOKE_HTTP_RETRY_DELAY`），降低网络抖动导致的误报

### 轻微问题 (P3)

- UI-002: 空字符串显示样式与文档描述不完全一致（应增加视觉区分）
- PMTiles: 无 Range 请求时完整下载可能内存压力，考虑流式响应
- 前端 CSS: `.preview-page`, `.spinner`, `.badge` 等样式未定义

## Windows Desktop Experience

### System Tray (Phase 1)

- [ ] 添加 tray-item 依赖（Windows only）
- [ ] 创建托盘模块 (tray.rs)
- [ ] 托盘菜单：打开 Web 界面
- [ ] 托盘菜单：退出 → 优雅关闭 + checkpoint
- [ ] Windows GUI 子系统配置（无控制台窗口）
- [ ] 托盘图标（当前为 placeholder，需替换）
- [ ] 手动测试：托盘退出 → 重启无 WAL 错误

### Future

- [ ] 日志文件输出（GUI 模式无控制台）
- [ ] 托盘菜单：打开日志文件
- [ ] 托盘菜单：显示服务状态（运行中/已停止）
- [ ] 托盘菜单：关于对话框

## Smoke Test Expansion

当前覆盖（scripts/smoke/）：
- [x] 服务启动 + 健康检查
- [x] 认证初始化 + 登录
- [x] GeoJSON 上传 + 处理
- [x] 私有瓦片获取
- [x] 发布 + 公开瓦片

待扩展：
- [ ] Shapefile 上传测试
- [ ] MBTiles 上传测试（MVT/PNG）
- [ ] 错误场景：无效格式、超大文件
- [ ] Schema 查询验证
- [ ] 特征属性端点验证
- [ ] CRS 更新验证
- [ ] Windows 托盘功能（手动测试自动化）
