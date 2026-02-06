# Testing & Quality Assurance

This document outlines the system behaviors and the strategy to verify them.

## Verification Philosophy

**Behaviors are contracts; tests are implementations.**

1.  **Rule of Choice:** Verify each behavior at the **lowest appropriate layer** (Unit > Integration/API > E2E) to ensure speed and stability.
2.  **Role of E2E:** Use E2E tests **ONLY** for critical "User Journeys" (cross-boundary flows) and browser-specific interactions. Do not use E2E for edge cases covered by lower layers.
3.  **Manual Fallback:** Manual verification is allowed only as a temporary measure and must be documented.

## 1. Critical User Journeys (E2E Required)

These flows ensure the system works end-to-end for the user. They must be covered by Playwright tests.

| Journey | Trigger | Expected Outcome |
| :--- | :--- | :--- |
| **Complete Upload Flow (GeoJSON)** | User uploads valid `.geojson` | List updates -> Status moves to `ready` -> Details accessible -> Preview opens map |
| **Complete Upload Flow (Shapefile)** | User uploads valid `.zip` | List updates -> Status moves to `ready` -> Details accessible -> Preview opens map |
| **Persistence across Restart** | Server restart after upload | Previously uploaded files remain visible and accessible in list/preview |
| **Preview Integration** | User clicks "Open Preview" | New tab opens, map loads, tile requests succeed (200 OK & non-empty) |

## 2. Behavioral Contracts (Layered Verification)

These specific behaviors should be verified by the most efficient means possible.

| Behavior | Trigger | Expected Outcome | Suggested Layer |
| :--- | :--- | :--- | :--- |
| **Validation (Format)** | Upload `.txt` / invalid ext | Reject with clear error | **API / Integration** |
| **Validation (Size)** | Upload file > Limit | Reject with clear error | **API / Integration** |
| **Validation (Zip Structure)** | Upload invalid `.zip` | Reject with clear error | **Unit / API** |
| **Status State Machine** | File processing lifecycle | Status transitions: `uploading` -> `uploaded` -> `processing` -> `ready` | **Integration** |
| **Details Metadata** | Request file details | Returns correct CRS, size, type | **API** |
| **Tile Generation** | Request specific MVT tile | Returns valid binary MVT with correct geometry | **Unit / Integration** |

## Testing Strategy & Commands

### E2E Tests (Playwright)
*Focus: User Journeys, Browser Interaction, Routing.*
- **Location:** `frontend/tests/`
- **Command:** `npm --prefix frontend run test:e2e`
- **Environment:** Each worker uses a distinct `PORT` and `DB_PATH`.

### API / Integration Tests (Rust)
*Focus: HTTP Contracts, DB State, File Processing, Validation.*
- **Location:** `backend/tests/` (Integration) or `backend/src/**` (Unit)
- **Command:** `cargo test`

### Release Safety Gate
- **Requirement:** Release builds must NOT expose test endpoints.
- **Verification:** CI builds release binary, starts it, and asserts `POST /api/test/reset` returns 404/405.

## Code Quality Standards (CI Gates)

| Language | Check Command |
| :--- | :--- |
| **Rust** | `cargo fmt -- --check` && `cargo clippy -- -D warnings` |
| **Frontend** | `biome format .` (or `npm run format:check`) |
