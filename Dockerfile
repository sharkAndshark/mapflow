# Stage 1: Build Frontend
FROM node:25-alpine AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build Backend
FROM rust:1.94-slim-bookworm AS backend-builder
WORKDIR /app
# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev g++ curl gzip && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml ./
COPY backend/Cargo.toml ./backend/
COPY Cargo.lock ./
COPY backend/extensions/spatial-extension-manifest.json ./backend/extensions/
# Create a minimal target so Cargo can parse the manifest.
# This keeps dependency caching without mutating the lockfile.
RUN mkdir -p backend/src && echo "fn main() {}" > backend/src/main.rs

# Pre-fetch deps for reproducible builds
RUN cargo fetch --locked --manifest-path backend/Cargo.toml

# Download spatial extension for embedding (required at build time)
ARG TARGETARCH
ARG SPATIAL_EXTENSION_ARCHIVE_URL
RUN set -eu; \
  archive_url="${SPATIAL_EXTENSION_ARCHIVE_URL:-}"; \
  if [ -z "${archive_url}" ]; then \
    duckdb_core_version="$(sed -n 's/.*"duckdb_core_version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' backend/extensions/spatial-extension-manifest.json | head -n 1)"; \
    if [ -z "${duckdb_core_version}" ]; then \
      duckdb_core_version="$(sed -n 's/.*"duckdb_version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' backend/extensions/spatial-extension-manifest.json | head -n 1)"; \
    fi; \
    if [ -z "${duckdb_core_version}" ]; then \
      echo "failed to parse duckdb_core_version (or fallback duckdb_version) from spatial-extension-manifest.json" >&2; \
      exit 1; \
    fi; \
    case "${TARGETARCH:-}" in \
      amd64) duckdb_platform="linux_amd64" ;; \
      arm64) duckdb_platform="linux_arm64" ;; \
      *) \
        echo "unsupported TARGETARCH for spatial extension auto-resolution: ${TARGETARCH:-unknown}" >&2; \
        exit 1 ;; \
    esac; \
    archive_url="http://extensions.duckdb.org/v${duckdb_core_version}/${duckdb_platform}/spatial.duckdb_extension.gz"; \
  fi; \
  curl -fsSL "${archive_url}" -o /tmp/spatial.duckdb_extension.gz; \
  gunzip -c /tmp/spatial.duckdb_extension.gz > backend/extensions/spatial.duckdb_extension; \
  rm -f /tmp/spatial.duckdb_extension.gz

# Build actual backend (locked)
COPY backend/src ./backend/src
RUN cargo build --release --locked --manifest-path backend/Cargo.toml --features embed-spatial-extension

# Stage 3: Runtime
FROM debian:bookworm-slim
WORKDIR /app
# Install runtime dependencies
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy artifacts
COPY --from=frontend-builder /app/frontend/dist ./dist
COPY --from=backend-builder /app/target/release/backend ./backend

# Environment setup
ENV WEB_DIST=/app/dist
ENV UPLOAD_DIR=/app/uploads
ENV DB_PATH=/app/data/mapflow.duckdb
ENV PORT=3000

# Create directories
RUN mkdir -p /app/uploads /app/data

EXPOSE 3000

CMD ["./backend"]
