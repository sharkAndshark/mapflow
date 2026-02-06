# System Behaviors & Contracts

This document defines the technical contracts, API specifications, and storage conventions for MapFlow. It is the reference for "what the system does" during development sprints.

## Current Sprint Scope
**Goal:** Allow data curators to upload, list, and preview spatial data files (Explorer Mode).

### Supported Formats
- **Shapefile:** Must be a `.zip` archive containing `.shp`, `.shx`, `.dbf`.
- **GeoJSON:** Standard `.geojson` file (single file).

## API Contracts

### Uploads
- `POST /api/uploads`
  - **Body:** `multipart/form-data` with `file` field.
  - **Constraints:** Max size defined by `UPLOAD_MAX_SIZE_MB`.
  - **Response (Success):** JSON with file metadata.
  - **Response (Error):** 400 Bad Request for invalid formats or size limit.

### File Management
- `GET /api/files`
  - **Returns:** List of files with `id`, `name`, `type`, `size`, `uploadedAt`, `status`, `crs`.

### Map Preview (Draft)
- `GET /api/files/:id/preview`
  - **Returns:** `id`, `name`, `crs`, `bbox` ([minx, miny, maxx, maxy], WGS84).
- `GET /api/files/:id/tiles/:z/:x/:y`
  - **Returns:** `application/vnd.mapbox-vector-tile` (MVT).
  - **Logic:** No feature properties included (geometry only) for performance.

### Testing Endpoints (Debug Only)
- `POST /api/test/reset`
  - **Behavior:** Resets database and storage.
  - **Security:** Available ONLY in debug builds with `MAPFLOW_TEST_MODE=1`. NEVER in release.

## Storage Conventions (DuckDB + Filesystem)

### Filesystem
- **Raw Files:** Stored in `./uploads/<id>/` (controlled by `UPLOAD_DIR`).

### Database Schema (DuckDB)
- **Table `files`:** Stores metadata (`id`, `name`, `path`, `status`, `error`, etc.).
- **Table `spatial_data`:** Stores imported geometry.

### Status Lifecycle
1. `uploading`: Frontend optimistic state.
2. `uploaded`: File received and saved to disk.
3. `processing`: Background import into DuckDB spatial table.
4. `ready`: Import complete, available for preview.
5. `failed`: Error occurred (details in `error` column).
   - **Recovery:** On server startup, any `processing` tasks are marked `failed`.

## Technical Implementation Details

### DuckDB Spatial Usage
- **CRS Detection:** `ST_Read_Meta(path)` to extract CRS auth code.
- **Reprojection:** `ST_Transform` to EPSG:3857 for tiles.
- **MVT Generation:** `ST_AsMVTGeom` (tile extent) -> `ST_AsMVT` (binary).
