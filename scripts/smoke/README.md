# Smoke Tests

对二进制和 Docker 镜像进行冒烟测试，覆盖核心用户场景。

## 测试场景

| 步骤 | 验证点 |
|------|--------|
| 1. 服务启动 | 健康检查通过 |
| 2. 认证初始化 | 创建管理员用户 |
| 3. 登录 | 会话 cookie 生效 |
| 4. 错误场景 | 无效格式上传返回 400（Unsupported file type） |
| 5. 上传矢量文件 | GeoJSON/Shapefile 上传成功 |
| 6. 状态轮询 | 矢量文件处理完成 (ready) |
| 7. Schema 查询 | `/api/files/:id/schema` 返回有效图层与字段 |
| 8. 特征属性查询 | `/api/files/:id/features/:fid` 返回可观测属性 |
| 9. CRS 更新 | `PUT /api/files/:id/crs` 生效并在 preview 元数据可观测 |
| 10. 获取瓦片 | 私有瓦片返回 MVT |
| 11. 发布文件 | 公开 slug 分配成功 |
| 12. 公开瓦片 | 无需认证访问瓦片 |
| 13. 上传 MBTiles(MVT) | MBTiles 上传成功 |
| 14. MBTiles(MVT) 元数据 | preview/meta 返回 `tileFormat=mvt` |
| 15. MBTiles(MVT) Schema 查询 | `/api/files/:id/schema` 返回有效图层 |
| 16. MBTiles(MVT) 特征属性 | `/api/files/:id/features/:fid` 返回 400（不支持） |
| 17. MBTiles(MVT) 发布 | 发布后 `tiles/:slug/meta` 返回 `tileFormat=mvt` |
| 18. 上传 MBTiles(PNG) | MBTiles 上传成功 |
| 19. MBTiles(PNG) 元数据 | preview/meta 返回 `tileFormat=png` |
| 20. MBTiles(PNG) Schema 查询 | `/api/files/:id/schema` 返回空图层数组 |
| 21. MBTiles(PNG) 瓦片类型 | 私有/公开瓦片 `Content-Type: image/png` |
| 22. 错误场景 | 超大文件上传返回 413（File too large） |

## 使用方法

### 二进制测试

```bash
# 基本用法
./scripts/smoke/smoke-binary.sh --binary ./target/release/backend

# 完整参数
./scripts/smoke/smoke-binary.sh \
  --binary ./target/release/backend \
  --port 3000 \
  --fixture frontend/tests/fixtures/sample.geojson \
  --mbtiles-fixture testdata/monaco_roads.mbtiles \
  --mbtiles-format mvt \
  --mbtiles-png-fixture testdata/sample_png.mbtiles \
  --mbtiles-png-format png \
  --expected-b64 testdata/smoke/expected_sample_z0_x0_y0.mvt.base64

# 保留测试数据（调试用）
SMOKE_KEEP_DATA=true ./scripts/smoke/smoke-binary.sh --binary ./mapflow
```

### Docker 测试

```bash
# 基本用法
./scripts/smoke/smoke-docker.sh --image mapflow:latest

# 完整参数
./scripts/smoke/smoke-docker.sh \
  --image ghcr.io/owner/mapflow:nightly \
  --port 3000 \
  --fixture frontend/tests/fixtures/sample.geojson \
  --mbtiles-fixture testdata/monaco_roads.mbtiles \
  --mbtiles-format mvt \
  --mbtiles-png-fixture testdata/sample_png.mbtiles \
  --mbtiles-png-format png \
  --expected-b64 testdata/smoke/expected_sample_z0_x0_y0.mvt.base64
```

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `SMOKE_PORT` | 服务端口 | 3000 |
| `SMOKE_FIXTURE` | 测试文件路径 | `frontend/tests/fixtures/sample.geojson` |
| `SMOKE_MBTILES_FIXTURE` | MBTiles 测试文件路径 | `testdata/monaco_roads.mbtiles` |
| `SMOKE_MBTILES_EXPECTED_FORMAT` | MBTiles 期望 tileFormat | `mvt` |
| `SMOKE_MBTILES_PNG_FIXTURE` | PNG MBTiles 测试文件路径 | `testdata/sample_png.mbtiles` |
| `SMOKE_MBTILES_PNG_EXPECTED_FORMAT` | PNG MBTiles 期望 tileFormat | `png` |
| `SMOKE_OVERSIZE_FIXTURE` | 超大文件测试路径（需大于限制） | `frontend/tests/fixtures/roads.zip` |
| `SMOKE_OVERSIZE_LIMIT_MB` | 超大文件测试时临时限制（通过 `/api/settings` 下调） | 1 |
| `SMOKE_CRS_UPDATE_INPUT` | CRS 更新请求值（PUT `/api/files/:id/crs`） | `urn:ogc:def:crs:EPSG::4490` |
| `SMOKE_CRS_UPDATE_EXPECTED` | 期望归一化 CRS | `EPSG:4490` |
| `SMOKE_CRS_UPDATE_EXPECTED_TYPE` | 期望 crsType | `standard` |
| `SMOKE_EXPECTED_B64` | 期望瓦片 base64 | (Docker: `testdata/smoke/...`) |
| `SMOKE_WORK_DIR` | 工作目录 | 临时目录 |
| `SMOKE_KEEP_DATA` | 保留测试数据 | false |
| `SMOKE_USERNAME` | 测试用户名 | smoke_admin |
| `SMOKE_PASSWORD` | 测试密码 | SmokePass1! |
| `SMOKE_HTTP_RETRIES` | HTTP 请求重试次数（网络抖动缓解） | 3 |
| `SMOKE_HTTP_RETRY_DELAY` | HTTP 请求重试间隔（秒） | 0.5 |

## 平台兼容性

- **Linux**: 原生 bash
- **macOS**: 原生 bash 或 bash via Homebrew
- **Windows**: Git Bash (GitHub Actions 默认支持)

## CI 集成

### nightly.yml / release.yml

```yaml
- name: Smoke test binary
  shell: bash
  run: |
    bash scripts/smoke/smoke-binary.sh \
      --binary "./target/${{ matrix.rust_target }}/release/backend${{ matrix.os == 'windows-latest' && '.exe' || '' }}"
```

### Docker smoke

```yaml
- name: Smoke test Docker
  run: bash scripts/smoke/smoke-docker.sh --image mapflow-smoke:ci
```

## 退出码

- `0`: 所有测试通过
- `1`: 测试失败（含详细错误信息）
