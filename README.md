# MapFlow (Explorer Edition)

<div align="center">

**A lightweight tool for data curators to upload, organize, and preview spatial data files**

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-supported-blue.svg)](https://www.docker.com/)
[![DuckDB](https://img.shields.io/badge/Powered%20by-DuckDB-yellow.svg)](https://duckdb.org/)

</div>

---

## 📋 Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Configuration](#configuration)
- [API Documentation](#api-documentation)
- [Development](#development)
- [Contributing](#contributing)
- [FAQ](#faq)
- [References](#references)

---

## 🎯 Overview

MapFlow Explorer Edition is a self-hosted spatial data management platform designed for data curators who need to:

- **Upload** and organize spatial datasets from various formats
- **Preview** data instantly on an interactive map
- **Manage** file metadata and access control

Built with DuckDB for efficient spatial data processing and OpenLayers for map visualization, MapFlow provides a clean, intuitive interface without the complexity of full GIS suites.

---

## ✨ Features

### 🔐 Secure Authentication
- Session-based user authentication
- bcrypt password hashing
- Configurable CORS and secure cookie settings

### 📤 Multi-Format Upload Support
| Format | Extensions | Notes |
|--------|-----------|-------|
| Shapefile | `.zip` | Must contain `.shp`, `.shx`, `.dbf` |
| GeoJSON | `.geojson`, `.json` | Standard GeoJSON |
| GeoJSONSeq | `.geojsonl`, `.geojsons` | Newline-delimited |
| KML | `.kml` | Keyhole Markup Language |
| GPX | `.gpx` | GPS Exchange Format |
| TopoJSON | `.topojson` | Topology-preserving format |
| MBTiles | `.mbtiles` | Pre-rendered tiles (Vector MVT & Raster PNG) |

### 📊 Data Management
- Simple file list with status tracking
- Schema extraction for supported formats
- Feature property queries

### 🗺️ Instant Map Preview
- Interactive map visualization
- Tile server for MBTiles
- Zoom and pan controls
- Feature inspection

---

## 🚀 Quick Start

### Prerequisites

- Docker Desktop or Docker Engine with Docker Compose v2

### Option 1: Prebuilt Image (Recommended)

```bash
docker compose -f docker-compose.ghcr.yml up -d
```

Access the application at: http://localhost:3000

### Option 2: Build Locally

```bash
docker compose up -d --build
```

### First-Time Setup

1. Open http://localhost:3000 in your browser
2. Create your admin account on the initialization page
3. **Password Requirements:**
   - Minimum 8 characters
   - At least one uppercase letter (A-Z)
   - At least one lowercase letter (a-z)
   - At least one digit (0-9)
   - At least one special character (!@#$%^&*)

4. Login with your credentials

### Stopping the Application

```bash
docker compose down
```

### Data Persistence

Data is stored in your project directory:
- `./data/` - DuckDB database
- `./uploads/` - Raw uploaded files

---

## 📖 Usage

### Uploading Files

Simply drag and drop a supported file onto the upload area, or click to browse:

- `.zip` (Shapefile bundle)
- `.geojson` / `.json` (GeoJSON)
- `.geojsonl` / `.geojsons` (GeoJSONSeq)
- `.kml` (KML)
- `.gpx` (GPX)
- `.topojson` (TopoJSON)
- `.mbtiles` (Tile packages)

### Viewing Data

1. Click any file in the list to view details
2. Click "Preview" to open the interactive map
3. For vector data, hover over features to see properties
4. Use zoom controls to navigate

---

## ⚙️ Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `UPLOAD_MAX_SIZE_MB` | `200` | Maximum file upload size (MB) |
| `PORT` | `3000` | Backend server port |
| `COOKIE_SECURE` | `false` | Set to `true` in production for HTTPS-only cookies |
| `CORS_ALLOWED_ORIGINS` | `http://localhost:3000` | Comma-separated list of allowed origins |

### Production Deployment

Create a `.env` file:

```env
CORS_ALLOWED_ORIGINS=https://mapflow.example.com,https://www.mapflow.example.com
COOKIE_SECURE=true
UPLOAD_MAX_SIZE_MB=500
```

Then start with:

```bash
docker compose up -d
```

---

## 🔌 API Documentation

### Public Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/auth/init` | Initialize system (first-time setup only) |
| `POST` | `/api/auth/login` | Login with username/password |
| `POST` | `/api/auth/logout` | Logout current user |
| `GET` | `/api/auth/check` | Check authentication status |

### Protected Endpoints (Require Authentication)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/files` | List all uploaded files |
| `POST` | `/api/uploads` | Upload a new file |
| `GET` | `/api/files/:id/preview` | Get file preview metadata |
| `GET` | `/api/files/:id/tiles/:z/:x/:y` | Get map tiles (XYZ format) |
| `GET` | `/api/files/:id/schema` | Get file schema information |
| `GET` | `/api/files/:id/features/:fid` | Get feature properties (DuckDB formats only) |

### Schema Endpoint Response

```json
{
  "layers": [
    {
      "id": "layer-name",
      "description": "Layer description (optional)",
      "fields": [
        {
          "name": "field_name",
          "type": "String|Number|Boolean|Date"
        }
      ]
    }
  ]
}
```

**Note:** For MBTiles vector tiles, schema is extracted from `metadata.json`. For raster tiles, an empty array is returned.

---

## 🛠️ Development

### Documentation

- [System Behaviors & Contracts](docs/dev/behaviors.md) - Design contracts and verification
- [Agent Collaboration Guidelines](AGENTS.md) - AI agent development guidelines

### Tech Stack

- **Backend:** Rust (implied from DuckDB + bcrypt)
- **Database:** DuckDB (embedded spatial database)
- **Frontend:** OpenLayers (map visualization)
- **Format Support:** GDAL (geospatial data conversion)

---

## 🤝 Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

---

## ❓ FAQ

### Q: What's the difference between Explorer Edition and other editions?

A: Explorer Edition is focused on data curation and preview. Future editions may include advanced analysis tools, collaborative features, and enterprise integrations.

### Q: Can I use MapFlow without Docker?

A: Currently, Docker is the recommended and supported deployment method. Native installation may be possible but is not officially documented.

### Q: How do I increase the upload size limit?

A: Set the `UPLOAD_MAX_SIZE_MB` environment variable to your desired value in MB.

### Q: Does MapFlow support user permissions?

A: Currently, MapFlow has a single admin account. Multi-user role management is planned for future releases.

### Q: Can I export my data?

A: Data export capabilities are planned. For now, you can access raw files in the `./uploads` directory.

---

## 🗺️ Roadmap

### Planned Enhancements

- [ ] MBTiles connection pooling for better performance
- [ ] Client-side feature property extraction from MVT tiles
- [ ] Multi-table format support (GeoPackage, File Geodatabase)
- [ ] Additional tile formats (WebP, TIFF)
- [ ] Tile caching layer
- [ ] Batch upload functionality
- [ ] Data export capabilities
- [ ] Multi-user roles and permissions

---

## 📚 References

### Specifications & Standards

- [MBTiles Specification](https://github.com/mapbox/mbtiles-spec) - Tile storage format
- [Vector Tile Specification](https://github.com/mapbox/vector-tile-spec) - MVT/PBF format
- [Tile Map Service (TMS)](https://wiki.osgeo.org/wiki/Tile_Map_Service_Specification) - OSGeo TMS spec

### Libraries & Tools

- [DuckDB](https://duckdb.org/) - Embedded analytical database
- [OpenLayers](https://openlayers.org/) - JavaScript map library
- [GDAL](https://gdal.org/) - Geospatial data library

---

## 📄 License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.

---

<div align="center">

**Made with ❤️ for the spatial data community**

[⬆ Back to Top](#mapflow-explorer-edition)

</div>
