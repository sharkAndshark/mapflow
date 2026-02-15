# Known Issues & TODOs

**Last Updated**: 2026-02-16

## Known Issues

### Flaky CI Tests (SIGSEGV)

- **Symptom**: `backend_tests` 偶发 `SIGSEGV` (signal 11) 进程崩溃
- **Evidence**:
  - 本地测试从未复现
  - CI 并行运行中一个成功一个失败（flaky）
  - 崩溃发生在 40+ 测试之后，不是特定测试
- **Root Cause**: DuckDB `bundled` feature 在 CI Linux 环境中的内存管理边缘情况
- **Workaround**: 可考虑 `cargo test -- --test-threads=2` 限制并发
- **Priority**: Low（非代码问题，等待 DuckDB 新版本）

## Performance Improvements

### Docker Build Time (docker_smoke)

- **Status**: ✅ Done (2026-02-16)
- **Solution**: Added `cache-from`/`cache-to` to ci.yml, sharing cache with release workflow
- **Expected**: ~1-2min with cache (from ~15min)

## Future Enhancements

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

### API 契约状态码不一致 (P1)

| 问题 | 文档 | 实现 | 建议 |
|------|------|------|------|
| API-001 返回码 | 200 | 201 (CREATED) | 更新文档为 201 |
| API-004/010 空瓦片 | 204 | 200 + 空 body | 统一为 204 或更新文档 |

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
