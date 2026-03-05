# Known Issues & TODOs

**Last Updated**: 2026-03-05

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
- [x] S13: Smoke 覆盖补齐超大文件错误场景（413）
  - 验证：`smoke-binary` 增加 `/api/settings` 下调上限 + 超限上传断言
  - 结果（2026-03-03）：✅ 通过
    - `bash -n scripts/smoke/lib/common.sh scripts/smoke/smoke-binary.sh scripts/smoke/smoke-docker.sh`
    - `bash scripts/smoke/smoke-binary.sh --binary ./target/debug/backend --port 3317`（日志包含 `upload max size to 1 MB` 与 `oversize upload rejected as expected`）
    - 备注：`smoke-docker.sh` 已实测通过（见 S17）
- [x] S14: Smoke 覆盖补齐 Schema 查询验证
  - 验证：`smoke-binary` 新增 `/api/files/:id/schema` 结构断言（layers/fields）
  - 结果（2026-03-03）：✅ 通过
    - `bash -n scripts/smoke/lib/common.sh scripts/smoke/smoke-binary.sh scripts/smoke/smoke-docker.sh`
    - `bash scripts/smoke/smoke-binary.sh --binary ./target/debug/backend --port 3317`（日志包含 `schema endpoint verified`）
    - 备注：`smoke-docker.sh` 已实测通过（见 S17）
- [x] S15: Smoke 覆盖补齐特征属性端点验证
  - 验证：`smoke-binary` 新增 `/api/files/:id/features/:fid` 结构断言（fid/properties）
  - 结果（2026-03-03）：✅ 通过
    - `bash -n scripts/smoke/lib/common.sh scripts/smoke/smoke-binary.sh scripts/smoke/smoke-docker.sh`
    - `bash scripts/smoke/smoke-binary.sh --binary ./target/debug/backend --port 3317`（日志包含 `feature properties endpoint verified`）
    - 备注：`smoke-docker.sh` 已实测通过（见 S17）
- [x] S16: Smoke 覆盖补齐 CRS 更新验证
  - 验证：`smoke-binary` 新增 `PUT /api/files/:id/crs` + preview 元数据回读断言（归一化 + crsType）
  - 结果（2026-03-03）：✅ 通过
    - `bash -n scripts/smoke/lib/common.sh scripts/smoke/smoke-binary.sh scripts/smoke/smoke-docker.sh`
    - `bash scripts/smoke/smoke-binary.sh --binary ./target/debug/backend --port 3317`（日志包含 `CRS update verified`）
    - 备注：`smoke-docker.sh` 已实测通过（见 S17）
- [x] S17: smoke-docker 实测 Shapefile 场景
  - 验证：Docker service 启动后，针对 `roads.zip` 运行完整容器冒烟链路
  - 结果（2026-03-03）：✅ 通过
    - `docker build -t mapflow-smoke:ci .`
    - `bash scripts/smoke/smoke-docker.sh --image mapflow-smoke:ci --port 3320 --fixture frontend/tests/fixtures/roads.zip --expected-b64 /tmp/nonexistent-smoke-expected.b64`
    - 关键日志：`status=ready` / `schema endpoint verified` / `feature properties endpoint verified` / `CRS update verified` / `oversize upload rejected as expected`
- [x] S18: Smoke 覆盖补齐 MBTiles(MVT) 元数据/Schema/错误语义
  - 验证：`smoke-binary` 与 `smoke-docker` 新增 MBTiles 场景（preview/public `tileFormat`、schema 非空、feature-properties=400）
  - 结果（2026-03-03）：✅ 通过
    - `bash -n scripts/smoke/lib/common.sh scripts/smoke/smoke-binary.sh scripts/smoke/smoke-docker.sh`
    - `bash scripts/smoke/smoke-binary.sh --binary ./target/debug/backend --port 3317`
    - `bash scripts/smoke/smoke-docker.sh --image mapflow-smoke:ci --port 3321 --fixture frontend/tests/fixtures/roads.zip --expected-b64 /tmp/nonexistent-smoke-expected.b64`
    - 关键日志：`MBTiles preview meta verified` / `MBTiles schema endpoint verified` / `MBTiles feature properties rejection verified` / `public tile meta verified`
- [x] S19: 恢复 spatial extension 编译期可选嵌入（避免 fresh checkout 构建回归）
  - 验证：默认构建不依赖 `backend/extensions/spatial.duckdb_extension`；release/self-contained 构建显式启用 `embed-spatial-extension`
  - 结果（2026-03-03）：✅ 通过
    - `cargo check --manifest-path backend/Cargo.toml --all-targets`
    - `cargo check --manifest-path backend/Cargo.toml --features embed-spatial-extension`
    - `cargo check --manifest-path backend/Cargo.toml --features embed-web-dist,embed-spatial-extension`
    - `cargo test --manifest-path backend/Cargo.toml resolve_candidates_prefers_explicit_env_path -- --nocapture`
    - `cargo test --manifest-path backend/Cargo.toml --features embed-spatial-extension write_embedded_spatial_extension_materializes_file -- --nocapture`
  - 备注：CI 新增 clean-check job（无预下载 extension）用于防止同类回归
- [x] S20: 修复 review 回归（Docker 自包含 + spatial 加载 fallback）
  - 验证：Docker 构建启用 `embed-spatial-extension`；本地 extension 加载失败不再提前 panic，优先尝试 `LOAD spatial` fallback
  - 结果（2026-03-03）：✅ 通过
    - `docker build -t mapflow-smoke:ci-fix .`
    - `bash scripts/smoke/smoke-docker.sh --image mapflow-smoke:ci-fix --port 3322 --fixture frontend/tests/fixtures/roads.zip --expected-b64 /tmp/nonexistent-smoke-expected.b64`
    - 关键日志：`Attempting to materialize embedded spatial extension` / `Spatial extension loaded successfully` / `SUCCESS: all smoke tests passed`
- [x] S21: 增加 spatial fallback 回归测试（坏本地文件不应阻断 `LOAD spatial`）
  - 验证：新增 `db::tests::ensure_spatial_extension_falls_back_after_local_load_failure`
  - 结果（2026-03-03）：✅ 通过
    - `cargo test --manifest-path backend/Cargo.toml ensure_spatial_extension_falls_back_after_local_load_failure -- --nocapture`
- [x] S22: Smoke 覆盖补齐 MBTiles(PNG) 场景（schema 空数组 + 私有/公开瓦片 content-type）
  - 验证：`smoke-binary` 与 `smoke-docker` 均覆盖 MVT+PNG 双场景
  - 结果（2026-03-03）：✅ 通过
    - `bash -n scripts/smoke/lib/common.sh scripts/smoke/smoke-binary.sh scripts/smoke/smoke-docker.sh`
    - `bash scripts/smoke/smoke-binary.sh --binary ./target/debug/backend --port 3331 --expected-b64 /tmp/nonexistent-smoke-expected.b64`
    - `docker build -t mapflow-smoke:mbtiles-png .`
    - `bash scripts/smoke/smoke-docker.sh --image mapflow-smoke:mbtiles-png --port 3332 --expected-b64 /tmp/nonexistent-smoke-expected.b64`

### 待执行（PostGIS 后续支持，按顺序）

> 备注：S23-S33 以 `feat/postgis-source-mvp` 合并主干后为基线推进。

- [ ] S23: PostGIS 连接复用与去重（避免每次注册都创建新连接记录）
  - 验证：同 connectionName 重复注册不同 source 时，`postgis_connections` 不重复增长
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_register_reuses_connection -- --nocapture`
- [ ] S24: PostGIS 私有瓦片路径支持 minZoom/maxZoom 限制（与公开路径语义一致）
  - 验证：`GET /api/files/:id/tiles/:z/:x/:y` 在越界时返回 204
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_private_tile_respects_zoom_limits -- --nocapture`
- [ ] S25: PostGIS `schema` / `features` 读取路径性能收敛（连接复用 + 查询超时）
  - 验证：批量读取时 p95 延迟稳定；超时返回明确错误语义
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_feature_query_timeout -- --nocapture`
- [ ] S26: SSL 模式扩展（`disable` -> `prefer/require`）
  - 验证：不同 sslMode 可成功建连；错误配置返回可诊断信息
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_ssl_modes -- --nocapture`
- [ ] S27: PostGIS function source v1（无参 set-returning function）
  - 验证：可注册 function 并走 preview/tile/schema/features/publish 全链路
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_function_source_noargs_lifecycle -- --nocapture`
- [ ] S28: PostGIS function source v2（参数化函数，白名单参数）
  - 验证：仅允许声明参数；禁止任意 SQL 注入路径
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_function_source_param_whitelist -- --nocapture`
- [ ] S29: 凭据管理增强（密钥版本化与轮换策略）
  - 验证：老版本加密凭据可平滑读取；支持迁移重加密
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_credential_key_rotation -- --nocapture`
- [ ] S30: PostGIS 观测性（结构化 tracing + 错误标签 + 查询耗时）
  - 验证：关键路径日志包含 source_id/connection_id/query_kind/duration_ms
  - 验证命令：`cargo test --manifest-path backend/Cargo.toml test_postgis_tracing_fields -- --nocapture`
- [ ] S31: PostGIS 集成测试基座（Docker PostGIS fixture）
  - 验证：CI 可稳定启动 fixture 并跑后端集成测试
  - 验证命令：`bash scripts/test/postgis-integration.sh`
- [ ] S32: Frontend E2E 覆盖 PostGIS 注册与预览发布流程
  - 验证：UI 从“连接 PostGIS”到公开 URL 全链路可观测
  - 验证命令：`npm --prefix frontend run test:e2e -- --grep \"PostGIS\"`
- [ ] S33: 契约文档补齐（`docs/dev/behaviors.md` + `docs/internal.md`）
  - 验证：新增 API/行为与实现一致，引用 lint 通过
  - 验证命令：`bash scripts/ci/lint_behaviors_refs.sh`

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

### Preview Tile Grid Alignment (2026-03-03)

- **Issue**: Preview 页 `TileDebug` 图层在初始化时使用默认 source，custom CRS 场景未绑定 custom `tileGrid`，导致网格显示错位或不显示。
- **Root Cause**: `Show Tile Grid` 仅切换图层可见性，但没有在 `meta` 变化后同步 `TileDebug` 的 projection/tileGrid。
- **Fix**:
  - 在 `Preview.jsx` 中复用 custom CRS 数据图层的 `Projection` + `TileGrid` 配置更新 `TileDebug` source
  - 保持 `Show Tile Grid` 手动开关默认关闭，不改 UX
  - 新增 E2E 可观测断言验证 `TileDebug` 图层对 custom CRS 的 `extent/origin/resolutions`

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
- [x] TileDebug 图层复用 custom `tileGrid`，确保 Preview 勾选 `Show Tile Grid` 后网格与 custom CRS 瓦片对齐

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
- [x] E2E: `Show Tile Grid` 在 custom CRS 下使用正确的 `extent/origin/resolutions`
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
| `zoom-limit.spec.js` | - | ✅ 已移除固定等待并补齐真实缩放约束验证 | 使用可观测交互 + tile z 层级断言 |
| `publish.spec.js` | - | ✅ 已移除固定等待 | 使用可观测 UI/接口状态断言 |

### 缺失的 E2E 测试覆盖 (P2)

| 契约 | 缺失验证 |
|------|---------|
| UI-001 | ✅ 已补充（2026-03-02）：`frontend/tests/polling.spec.js` 新增 `preview action is hidden before ready and shown after ready` |
| UI-003 | ✅ 已补充（2026-03-02）：`frontend/tests/preview.spec.js` 新增 `click feature switches highlight style immediately`（点击要素后验证 selected/default 样式切换） |
| UI-010 | ✅ 已补充（2026-03-02）：`frontend/tests/zoom-limit.spec.js` 新增基于真实缩放交互 + tile z 观测的约束验证 |

### CI/CD 改进 (P2)

1. **CI 触发降噪**: ✅ 已完成（2026-03-02）  
   `ci.yml` 调整为 `push` 仅 `main` 触发，开发分支通过 `pull_request` 触发；并增加 workflow `concurrency` 自动取消旧 run，减少 PR 期间重复计算
2. **供应链安全**: ✅ 已完成（2026-03-02）  
   DuckDB spatial extension 下载链路已增加 archive SHA256 校验（manifest + release 脚本）
3. **冒烟测试健壮性**: ✅ 已完成（2026-03-02）  
   `scripts/smoke/lib/common.sh` 已为关键 HTTP 调用增加可配置重试（`SMOKE_HTTP_RETRIES` / `SMOKE_HTTP_RETRY_DELAY`），降低网络抖动导致的误报

### 轻微问题 (P3)

- UI-002: 空字符串显示样式与文档描述不完全一致（应增加视觉区分）
- PMTiles: 无 Range 请求时完整下载可能内存压力，考虑流式响应
- 前端 CSS: `.preview-page`, `.spinner`, `.badge` 等样式未定义

## Windows Desktop Experience

### Tray + Console 关闭路径设计分析（2026-03-03）

- **结论**：WAL 风险不是 Windows 独有；Windows 特有问题是“托盘模型”和“可直接关闭控制台窗口”并存，导致退出路径不唯一。
- **现状事实**：
  - 2026-03-04 起采用双入口：`mapflow-desktop.exe`（GUI+tray）+ `backend.exe`（console/dev）。
  - Windows 发布包仅暴露 desktop 入口，终端用户默认不再接触可直接关闭的控制台窗口。
  - console 入口新增控制台关闭事件处理（`CTRL_CLOSE_EVENT/CTRL_LOGOFF_EVENT/CTRL_SHUTDOWN_EVENT`）并尝试走 checkpoint。
  - `db::open_with_wal_recovery` 现在是跨平台兜底（WAL 相关 open/replay 错误时隔离并重试），但它只解决“下次能启动”，不能保证“最后一次未 checkpoint 写入不丢”。
- **自动化覆盖现状**：
  - `ci.yml` 目前只在 Linux 跑 backend/frontend 测试。
  - `nightly.yml`/`release.yml` 的 binary smoke 已包含 `windows-latest` 矩阵。
- **设计方向**：
  - ✅ 已落地：Windows 桌面发布收敛为单一退出路径：GUI 子系统（无控制台窗口）+ tray Exit。
  - ✅ 已落地：开发/CLI 场景保留 console 模式，并补控制台关闭信号处理，减少“直接关窗”硬退出窗口。

### System Tray (Phase 1)

- [x] 添加 tray-item 依赖（Windows only）
- [x] 创建托盘模块 (tray.rs)
- [x] 托盘菜单：打开 Web 界面
- [x] 托盘菜单：退出 → 优雅关闭 + checkpoint
- [x] Windows GUI 子系统配置（desktop binary，无控制台窗口）
- [ ] 托盘图标（当前为 placeholder，需替换）
- [ ] 手动测试：托盘退出 → 重启无 WAL 错误
- [x] WAL 启动恢复加固（跨平台）：WAL 相关 open/replay 错误时备份隔离 `*.wal.bak.<ts>` 并重试；支持非 `.duckdb` 的 `DB_PATH` 推导与 `WAL_RECOVERY_STRICT=1`
- [x] 新增跨平台 crash-recovery smoke（本地 + workflow）：`启动 -> 写入 -> 强制终止 -> 重启`，断言服务可恢复启动且有明确日志（`scripts/smoke/smoke-crash-recovery.sh` + nightly/release binary matrix）
- [ ] nightly/release 增加 Windows 强制终止恢复 smoke（PowerShell）并归档故障日志（含 `.wal.bak.*`）
- [x] 评估并落地 Windows 双模式启动：`desktop(gui+tray)` 与 `console(dev)`，避免发布形态暴露“直接关控制台”路径

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
- [x] Shapefile 上传测试（`scripts/smoke/smoke-docker.sh --fixture frontend/tests/fixtures/roads.zip`）
- [x] MBTiles 上传测试（MVT）（`scripts/smoke/smoke-binary.sh` / `scripts/smoke/smoke-docker.sh` 默认覆盖）
- [x] MBTiles 上传测试（PNG）（`scripts/smoke/smoke-binary.sh` / `scripts/smoke/smoke-docker.sh` 默认覆盖；含 schema 空数组与 private/public `Content-Type: image/png` 断言）
- [x] 错误场景：无效格式上传返回 400（`scripts/smoke/smoke-binary.sh` / `scripts/smoke/smoke-docker.sh`）
- [x] 错误场景：超大文件（413）（`scripts/smoke/smoke-binary.sh` / `scripts/smoke/smoke-docker.sh`）
- [x] Schema 查询验证（`scripts/smoke/smoke-binary.sh` / `scripts/smoke/smoke-docker.sh`）
- [x] 特征属性端点验证（`scripts/smoke/smoke-binary.sh` / `scripts/smoke/smoke-docker.sh`）
- [x] CRS 更新验证（`scripts/smoke/smoke-binary.sh` / `scripts/smoke/smoke-docker.sh`）
- [ ] Windows 托盘功能（手动测试自动化）
