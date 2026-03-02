# Smoke Tests

对二进制和 Docker 镜像进行冒烟测试，覆盖核心用户场景。

## 测试场景

| 步骤 | 验证点 |
|------|--------|
| 1. 服务启动 | 健康检查通过 |
| 2. 认证初始化 | 创建管理员用户 |
| 3. 登录 | 会话 cookie 生效 |
| 4. 上传文件 | GeoJSON 上传成功 |
| 5. 状态轮询 | 文件处理完成 (ready) |
| 6. 获取瓦片 | 私有瓦片返回 MVT |
| 7. 发布文件 | 公开 slug 分配成功 |
| 8. 公开瓦片 | 无需认证访问瓦片 |

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
  --expected-b64 testdata/smoke/expected_sample_z0_x0_y0.mvt.base64
```

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `SMOKE_PORT` | 服务端口 | 3000 |
| `SMOKE_FIXTURE` | 测试文件路径 | `frontend/tests/fixtures/sample.geojson` |
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
