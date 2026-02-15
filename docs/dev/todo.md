# Known Issues & TODOs

**Last Updated**: 2026-02-16

## Performance Improvements

### Docker Build Time (docker_smoke)

- **Status**: ✅ Done (2026-02-16)
- **Solution**: Added `cache-from`/`cache-to` to ci.yml, sharing cache with release workflow
- **Expected**: ~1-2min with cache (from ~15min)

## Future Enhancements

### MBTiles Connection Pool

- **Current**: New SQLite connection per request
- **Goal**: Connection pooling for high-traffic scenarios
- **Priority**: High (for production)

### Slug Race Condition

- **Current**: Manual uniqueness check before INSERT
- **Issue**: Small probability of race condition
- **Solution Options**:
  1. Database unique constraint (requires DuckDB support)
  2. Transaction with retry logic
- **Priority**: Low (acceptable for current phase)
