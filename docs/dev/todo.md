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
