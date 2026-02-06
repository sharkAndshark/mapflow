# Testing & Quality Assurance

This document outlines the observable behaviors used to verify system correctness.

## Behavior Checklist (Observable Contracts)

These behaviors must be verified by E2E tests (or manual regression) before merging.

| Behavior | Trigger | Expected Outcome |
| --- | --- | --- |
| **Upload (GeoJSON)** | UI upload `.geojson` | List shows file, status becomes "ready" |
| **Upload (Shapefile)** | UI upload `.zip` | List shows file, status becomes "ready" |
| **Persistence** | Restart server | Previously uploaded files remain visible in list |
| **Validation (Format)** | Upload `.txt` or invalid ext | Error message displayed |
| **Validation (Size)** | Upload file > Limit | Error message displayed |
| **Validation (Structure)** | Upload invalid `.zip` | Error message displayed |
| **Preview** | Click "Open Preview" | Opens new tab, map loads, tiles request returns 200 |
| **Details** | Click list item | Sidebar shows metadata (CRS, size, etc.) |
| **Status Update** | Upload large file | Status transitions: uploaded -> processing -> ready |

## Testing Strategy

### E2E Tests (Playwright)
- **Location:** `frontend/tests/` (or similar).
- **Command:** `npm --prefix frontend run test:e2e`
- **Environment:** Each worker uses a distinct `PORT` and `DB_PATH` to prevent conflicts.

### Release Safety Gate
- **Requirement:** Release builds must NOT expose test endpoints.
- **Verification:** CI builds release binary, starts it, and asserts `POST /api/test/reset` returns 404/405.

## Code Quality Standards (CI Gates)

| Language | Check Command |
| --- | --- |
| **Rust** | `cargo fmt -- --check` && `cargo clippy -- -D warnings` |
| **Frontend** | `biome format .` (or `npm run format:check`) |
