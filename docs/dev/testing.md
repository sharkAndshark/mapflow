# Testing & Quality Assurance

This document outlines the system behaviors and the strategy to verify them.

## Verification Philosophy

**Behaviors are contracts; tests are implementations.**

- **Observable Contracts, Not Internals:** Tests assert externally observable behavior (API responses, state transitions, persistence, visible UI changes), not implementation details.
- **Lowest Appropriate Layer Wins:** Default to the fastest deterministic layer that can prove the behavior (Unit > Integration (HTTP/API) > E2E).
- **E2E Is a Scarce Budget:** Use E2E only when the contract is cross-boundary (journeys across browser/server/persistence) or requires browser/runtime integration. Do not use E2E to cover edge-case matrices.
- **Design for Testability:** If a behavior is only testable via slow/flaky paths, change the design (introduce seams, extract pure logic, make async flows awaitable) rather than piling on E2E.
- **Determinism Over Timing:** Avoid arbitrary sleeps. Prefer explicit readiness signals and awaitable completion; use bounded polling only as a fallback.
- **Missing Layers Are Debt:** If the codebase lacks a fast layer for the behavior (e.g. no component/unit harness), treat that as debt to pay down, not a reason to move everything up to E2E.

- **Manual Fallback (Temporary):** Manual verification is allowed only as a temporary measure and must be documented.

### Red Flags

- Tests duplicate implementation (re-implementing algorithms/protocols/parsers just to assert outcomes).
- Tests lock onto unstable details (copy, formatting, time strings, CSS structure) instead of stable contracts, unless the copy itself is the contract.
- Most new behaviors only have E2E coverage or only have heavy integration coverage.

### Refactor Triggers

Refactoring is warranted when it reduces change amplification and test friction while preserving observable contracts.

- **Hard triggers:** the same business rule is duplicated in multiple places; a behavior cannot be proven at a low layer (only E2E/heavy integration is feasible); tests require arbitrary sleeps to pass; fixing one behavior forces edits across multiple layers due to accidental coupling.
- **Soft signals:** a module keeps growing because responsibilities drift; error/status semantics become inconsistent across endpoints/UI; the suite gets slower/flakier over time and failures are harder to localize.

### Change Loop (Humans & Agents)

- Start from contracts: if behavior changes, update the observable contract first (API/status/error semantics/UI observables).
- Prove the change at the lowest appropriate layer with a deterministic test; add E2E only when the user journey is the contract.
- If tests are only possible via slow/flaky paths, refactor for testability (seams, pure logic, awaitable completion).
- Keep the suite localizable: prefer assertions that identify which layer broke.
- After implementation, update this document and `docs/dev/behaviors.md` when contracts or verification strategy change.

### Definition of Done (Quality)

- Any behavior change has at least one deterministic test at the lowest appropriate layer.
- Contract changes are reflected in `docs/dev/behaviors.md` and in tests.
- E2E coverage stays small and focuses on critical journeys, not edge-case matrices.
- The change reduces (or does not increase) change amplification and test friction.

## Critical User Journeys (E2E Required)

These flows ensure the system works end-to-end for the user. They must be covered by Playwright tests.

| Journey | Trigger | Expected Outcome |
| :--- | :--- | :--- |
| **Complete Upload Flow (GeoJSON)** | User uploads valid `.geojson` | List updates -> Status moves to `ready` -> Details accessible -> Preview opens map |
| **Complete Upload Flow (Shapefile)** | User uploads valid `.zip` | List updates -> Status moves to `ready` -> Details accessible -> Preview opens map |
| **Persistence across Restart** | Server restart after upload | Previously uploaded files remain visible and accessible in list/preview |
| **Preview Integration** | User clicks "Open Preview" | New tab opens, map loads, tile requests succeed (200 OK & non-empty) |

## Behavioral Contracts (Layered Verification)

These specific behaviors should be verified by the most efficient means possible.

| Behavior | Trigger | Expected Outcome | Suggested Layer |
| :--- | :--- | :--- | :--- |
| **Validation (Format)** | Upload `.txt` / invalid ext | Reject with clear error | **Integration (HTTP/API)** |
| **Validation (Size)** | Upload file > Limit | Reject with clear error | **Integration (HTTP/API)** |
| **Validation (Zip Structure)** | Upload invalid `.zip` | Reject with clear error | **Unit / Integration (HTTP/API)** |
| **Status State Machine** | File processing lifecycle | Status transitions: `uploading` -> `uploaded` -> `processing` -> `ready` | **Integration** |
| **Details Metadata** | Request file details | Returns correct CRS, size, type | **Integration (HTTP/API)** |
| **Tile Generation** | Request specific MVT tile | Returns valid binary MVT with correct geometry and properties (tags) | **Unit / Integration** |

## Testing Strategy & Commands

### E2E Tests (Playwright)
*Focus: User Journeys, Browser Interaction, Routing.*
- **Location:** `frontend/tests/`
- **Command:** `npm --prefix frontend run test:e2e`
- **Environment:** Each worker uses a distinct `PORT` and `DB_PATH`.

Note: The frontend uses Playwright E2E for critical journeys and a small Vitest unit-test layer for pure logic. It still lacks a component/unit harness for UI contracts; treat that as technical debt and pull coverage down from E2E over time.

### Integration Tests (Rust, HTTP/API)
*Focus: HTTP Contracts, DB State, File Processing, Validation.*
- **Location:** `backend/tests/` (Integration) or `backend/src/**` (Unit)
- **Note:** `backend/tests/` is the canonical place for HTTP/API contract tests; `backend/src/**` unit tests are for pure helpers.
- **Command:** `cargo test`

Note: When asserting MVT contents (properties/tags), prefer using a mature decoder (e.g. `mvt-reader`) over hand-rolled protobuf parsing.

### Release Safety Gate
- **Requirement:** Release builds must NOT expose test endpoints.
- **Verification:** CI builds release binary, starts it, and asserts `POST /api/test/reset` returns 404/405.

## Code Quality Standards (CI Gates)

| Language | Check Command |
| :--- | :--- |
| **Rust** | `cargo fmt -- --check` && `cargo clippy -- -D warnings` |
| **Frontend** | `biome format .` (or `npm run format:check`) |

## Working Agreement (Local)

- Before committing, run at least one test suite relevant to the change.
  - Backend-only refactors: `just test-backend`
  - Frontend-only changes: run `npm --prefix frontend run format:check` + `npm --prefix frontend run test:unit` + `npm --prefix frontend run build`. Run E2E when changing critical user journeys (`npm --prefix frontend run test:e2e`).
  - Cross-cutting / uncertain impact: `just test`
- Note: repository hooks also run tests automatically (commit: backend tests + frontend unit; push: full `just test` including E2E). This is a safety net, not a replacement for intentionally running the most relevant suite while iterating.
