# MapFlow (Explorer Edition)

**MapFlow** is a lightweight tool for data curators to upload, organize, and preview spatial data files.

## Features
- **Upload:** Support for Shapefile (zipped) and GeoJSON.
- **Manage:** Simple file list with status tracking.
- **Preview:** Instant map preview for uploaded datasets.

## Quickstart

### Installation
(Instructions for future binary release or docker pull)

### Usage
1. Start the application.
2. Open `http://localhost:3000` in your browser.
3. Drag and drop a `.zip` (Shapefile) or `.geojson` file to upload.
4. Click a file in the list to view details or open the map preview.

## Development

**For Contributors & AI Agents:**
- **Entry Point:** See [docs/dev.md](./docs/dev.md) for architecture, setup, and workflows.
- **Behaviors:** See [docs/dev/behaviors.md](./docs/dev/behaviors.md) for API contracts.
- **Testing:** See [docs/dev/testing.md](./docs/dev/testing.md) for verification checklists.
- **Commands:**
  - `just start` (Run app)
  - `cargo test` / `npm test`
  - `cargo clippy` / `cargo fmt`
