# MapFlow (Explorer Edition)

**MapFlow** is a lightweight tool for data curators to upload, organize, and preview spatial data files.

License: Apache-2.0

## Features
- **Upload:** Support for Shapefile (zipped) and GeoJSON.
- **Manage:** Simple file list with status tracking.
- **Preview:** Instant map preview for uploaded datasets.

## Quickstart

### Run With Docker (Recommended)

Prerequisites: Docker Desktop (or Docker Engine) with Docker Compose v2.

Option 1: Run the prebuilt image from GHCR (recommended for users):

```bash
docker compose -f docker-compose.ghcr.yml up -d
```

If you fork the repo or publish under a different image name:

```bash
MAPFLOW_IMAGE=ghcr.io/<your-org-or-user>/mapflow:latest docker compose -f docker-compose.ghcr.yml up -d
```

Option 2: Build locally with Docker:

```bash
docker compose up -d --build
```

Data is persisted on your machine (same folder as the compose file):
- `./data` (DuckDB)
- `./uploads` (raw uploads)

To stop:

```bash
docker compose down
```

### Usage
1. Start the application.
2. Open `http://localhost:3000` in your browser.
3. Drag and drop a `.zip` (Shapefile) or `.geojson` file to upload.
4. Click a file in the list to view details or open the map preview.

### Configuration

Environment variables (Docker):
- `UPLOAD_MAX_SIZE_MB` (default: `200`)
- `PORT` (default: `3000`)

## Development

See `docs/dev.md`.
