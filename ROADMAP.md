# MapFlow Roadmap

This is the single source of truth for planning and evolving MapFlow. All feature work should be proposed, discussed, and accepted here.

## 1. Vision
MapFlow is a high-performance map tile server with a visual, node-based editor that lets users publish XYZ tile services by drag-and-drop.

## 2. Current State (Snapshot)
- Core pipeline exists: upload -> resource -> layer -> xyz -> validate/apply -> tiles.
- UI editor supports node creation, linking, and property editing.
- Upload validates shapefile structure and SRID (with override).
- Config validation is strict and consistent with node graph rules.
- Tiles endpoint works but needs performance stabilization on large datasets.

## 3. Short-Term Plan (Next 2 Iterations)
### Iteration A (Stability)
- [ ] Stabilize tile generation for large datasets (latency + reliability).
- [ ] Add user-facing upload errors (clear feedback in UI).
- [ ] Add lightweight observability for slow tile requests.

### Iteration B (Usability)
- [ ] Add a map preview mode or quick tile inspection (non-PBF preview).
- [ ] Provide a one-click “Create demo pipeline” action for new users.
- [ ] Improve node status cues (e.g., missing connections or invalid settings).

## 4. Backlog (Candidates)
- [ ] Tile caching and/or precomputation strategies.
- [ ] Layer styling metadata.
- [ ] Export/Import of full node graph configurations.
- [ ] Multi-tileset management.
- [ ] More robust SRID autodetection and coordinate transforms.

## 5. Acceptance Criteria
A feature is accepted when:
- It is described here first (briefly),
- The behavior is observable in the UI or API,
- Tests (unit/integration) are added or updated,
- Documentation is updated in README if user-facing.

## 6. Constraints & Risks
- DuckDB spatial performance can degrade with large tables.
- Tile queries are currently synchronous and can block.
- Large uploads require clear user feedback and progressive status.

## 7. Change Log (Rollups)
- 2026-02-03: MVP implemented with UI editor, config validation, upload, and tiles pipeline.
- 2026-02-03: Added XYZ node “Open Tile” quick button for manual diagnosis.
