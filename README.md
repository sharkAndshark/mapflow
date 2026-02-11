# MapFlow (Explorer Edition)

**MapFlow** is a lightweight tool for data curators to upload, organize, and preview spatial data files.

License: Apache-2.0

## Features
- **Authentication:** Secure user authentication with session-based login.
- **Upload:** Support for multiple spatial data formats:
  - Shapefile (zipped `.zip`)
  - GeoJSON (`.geojson`, `.json`)
  - GeoJSONSeq / Newline-Delimited GeoJSON (`.geojsonl`, `.geojsons`)
  - KML (`.kml`)
  - GPX (`.gpx`)
  - TopoJSON (`.topojson`)
  - MBTiles (`.mbtiles`) - Pre-rendered tile packages (vector MVT and raster PNG supported)
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

#### First-Time Setup

1. Start the application.
2. Open `http://localhost:3000` in your browser.
3. **Create an admin account** on the initialization page (first-time only).
   - Password must be at least 8 characters and include uppercase, lowercase, number, and special character.
4. **Login** with your admin credentials.

#### Daily Usage

1. Drag and drop a spatial data file to upload:
   - `.zip` (Shapefile with .shp/.shx/.dbf)
   - `.geojson` / `.json` (GeoJSON)
   - `.geojsonl` / `.geojsons` (GeoJSONSeq/NDJSON)
   - `.kml` (KML)
   - `.gpx` (GPX)
   - `.topojson` (TopoJSON)
   - `.mbtiles` (Pre-rendered tiles - supports both vector MVT and raster PNG formats)
2. Click a file in the list to view details or open the map preview.

### Configuration

Environment variables (Docker):
- `UPLOAD_MAX_SIZE_MB` (default: `200`) - Maximum file upload size in megabytes
- `PORT` (default: `3000`) - Backend server port
- `COOKIE_SECURE` (default: `false`) - Set to `true` in production to ensure session cookies are only transmitted over HTTPS
- `CORS_ALLOWED_ORIGINS` (default: `http://localhost:3000`) - Comma-separated list of allowed origins for CORS

**Important:** For production deployments, set `CORS_ALLOWED_ORIGINS` to your actual domain(s):

```bash
# Example for production
docker compose up -d -e CORS_ALLOWED_ORIGINS=https://mapflow.example.com,https://www.mapflow.example.com
```

Or create a `.env` file:

```bash
CORS_ALLOWED_ORIGINS=https://mapflow.example.com,https://www.mapflow.example.com
COOKIE_SECURE=true
```

## Development

- [behaviors.md](./docs/dev/behaviors.md) - System contracts and verification
- [AGENTS.md](./AGENTS.md) - Agent collaboration guidelines

## Authentication

MapFlow uses session-based authentication with secure password hashing (bcrypt).

### API Endpoints

**Public Endpoints** (no authentication required):
- `POST /api/auth/init` - Initialize system (first-time setup)
- `POST /api/auth/login` - Login with username/password
- `POST /api/auth/logout` - Logout current user
- `GET /api/auth/check` - Check authentication status

**Protected Endpoints** (require authentication):
- `GET /api/files` - List all files
- `POST /api/uploads` - Upload a new file
- `GET /api/files/:id/preview` - Get file preview metadata
- `GET /api/files/:id/tiles/:z/:x/:y` - Get map tiles
- `GET /api/files/:id/schema` - Get file schema
- `GET /api/files/:id/features/:fid` - Get feature properties

### Password Requirements

Passwords must meet the following complexity requirements:
- Minimum 8 characters
- At least one uppercase letter (A-Z)
- At least one lowercase letter (a-z)
- At least one digit (0-9)
- At least one special character (!@#$%^&* etc.)

### Session Management

- Sessions are stored in the database with automatic expiration
- Session cookies are HTTP-only and use secure flags in production
- Sessions persist across server restarts

## TODO

### Performance Optimizations

- **MBTiles Connection Pooling:** Currently, each tile request opens a new SQLite connection. For high-traffic deployments, implement a connection pool (e.g., using `r2d2` crate) to reuse connections and reduce overhead.

### Future Enhancements

- Support for additional tile formats (WebP, TIFF)
- Tile caching layer for improved performance
- Batch upload functionality
- Data export capabilities

## References

### Specifications & Standards

- **[MBTiles Specification](https://github.com/mapbox/mbtiles-spec)** - The MBTiles format specification for storing map tiles in SQLite databases
- **[Vector Tile Specification](https://github.com/mapbox/vector-tile-spec)** - Mapbox Vector Tile specification for MVT/PBF format
- **[Tile Map Service (TMS)](https://wiki.osgeo.org/wiki/Tile_Map_Service_Specification)** - OSGeo TMS specification (XYZ to TMS coordinate conversion)

### Libraries & Tools

- **[DuckDB](https://duckdb.org/)** - Embedded analytical database for spatial data processing
- **[OpenLayers](https://openlayers.org/)** - JavaScript library for displaying maps in web browsers
- **[GDAL](https://gdal.org/)** - Geospatial Data Abstraction Library for raster and vector data formats
