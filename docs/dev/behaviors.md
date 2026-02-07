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
  - **Response (Error):**
    - 400 Bad Request for invalid formats/structure.
    - 413 Payload Too Large when exceeding size limit.
    - Body: `{ "error": "..." }`.

### File Management
- `GET /api/files`
  - **Returns:** List of files with `id`, `name`, `type`, `size`, `uploadedAt`, `status`, `crs`, `path` and optional `error`.

### Map Preview (Draft)
- `GET /api/files/:id/preview`
  - **Returns:** `id`, `name`, `crs`, `bbox` ([minx, miny, maxx, maxy], WGS84).
  - **Response (Error):**
    - 404 Not Found when `:id` does not exist.
    - 409 Conflict when the file is not ready for preview (e.g. `uploaded`, `processing`, `failed`).
      - Body: `{ "error": "File is not ready for preview" }`.
- `GET /api/files/:id/tiles/:z/:x/:y`
  - **Returns:** `application/vnd.mapbox-vector-tile` (MVT).
  - **Logic:** Includes feature properties (tags) in addition to geometry.
  - **Constraints:**
    - `z` must be within `[0, 22]`.
    - `x` and `y` must be within `[0, 2^z - 1]`.
  - **Response (Error):**
    - 400 Bad Request when tile coordinates are invalid.
      - Body: `{ "error": "Invalid tile coordinates" }`.
    - 404 Not Found when `:id` does not exist.
    - 409 Conflict when the file is not ready for preview.
      - Body: `{ "error": "File is not ready for preview" }`.

### Feature Properties (Stable Schema)
- `GET /api/files/:id/features/:fid`
  - **Purpose:** Fetch a single feature's properties from DuckDB by its stable `fid`, using the dataset's captured column schema.
  - **Why:** MVT feature tags may omit NULL-valued properties; this endpoint guarantees a stable field list per dataset/table.
  - **Returns (Success):**
    - `{ "fid": <number>, "properties": [ { "key": <string>, "value": <json|null> }, ... ] }`
    - `properties` is ordered by the dataset column `ordinal`.
    - `key` uses original column names (user-facing).
    - `value` is `null` when the row value is NULL.
  - **Response (Error):**
    - 404 Not Found when `:id` does not exist.
      - Body: `{ "error": "File not found" }`.
    - 409 Conflict when the file is not ready for preview.
      - Body: `{ "error": "File is not ready for preview" }`.
    - 404 Not Found when `:fid` does not exist in the dataset.
      - Body: `{ "error": "Feature not found" }`.

### Testing Endpoints (Debug Only)
- `POST /api/test/reset`
  - **Behavior:** Resets database and storage.
  - **Security:** Available ONLY in debug builds with `MAPFLOW_TEST_MODE=1`. NEVER in release.

## Storage Conventions (DuckDB + Filesystem)

### Filesystem
- **Raw Files:** Stored in `./uploads/<id>/` (controlled by `UPLOAD_DIR`).

### Database Schema (DuckDB)
- **Table `files`:** Stores metadata (`id`, `name`, `path`, `status`, `error`, etc.).
- **Table `dataset_columns`:** Stores per-dataset column mapping (normalized identifier -> original property key) and MVT-compatible types.
- **Per-dataset tables:** Each upload is imported into its own DuckDB table and referenced by `files.table_name`.

### Status Lifecycle
1. `uploading`: Frontend optimistic state.
2. `uploaded`: File received and saved to disk.
3. `processing`: Background import into DuckDB spatial table.
4. `ready`: Import complete, available for preview.
5. `failed`: Error occurred (details in `error` column).
   - **Recovery:** On server startup, any `processing` tasks are marked `failed`.

Note: Backend currently persists `uploaded`, `processing`, `ready`, `failed`. The `uploading` state exists only on the frontend.

## UI Observable Contracts

### Preview Availability

- The UI MUST allow opening the preview page only when the selected file `status` is `ready`.
- When the file is not `ready` (`uploaded`, `processing`, `failed`), the UI MUST present the preview action as disabled (not clickable).

### Preview Feature Inspector

- For a given DuckDB table (dataset), the feature inspector MUST display a stable set of property fields based on the dataset schema.
- Fields with NULL values MUST still be shown (with a UI placeholder value).
- UI placeholder conventions:
  - NULL is displayed as `--`.
  - Empty strings are displayed as `""`.
  - Visual distinction: placeholder values are muted; NULL is also italic; hover tooltips clarify `NULL` vs `Empty string`.

## Technical Implementation Details

### DuckDB Spatial Usage
- **CRS Detection:** `ST_Read_Meta(path)` to extract CRS auth code.
- **Reprojection:** `ST_Transform` to EPSG:3857 for tiles.
- **MVT Generation:** `ST_AsMVTGeom` (tile extent) -> `ST_AsMVT` (binary).
