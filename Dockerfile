# Stage 1: Build Frontend
FROM node:25-alpine AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build Backend
FROM rust:1.93-slim-bookworm AS backend-builder
WORKDIR /app
# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev g++ && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml ./
COPY backend/Cargo.toml ./backend/
COPY Cargo.lock ./
# Create a minimal target so Cargo can parse the manifest.
# This keeps dependency caching without mutating the lockfile.
RUN mkdir -p backend/src && echo "fn main() {}" > backend/src/main.rs

# Pre-fetch deps for reproducible builds
RUN cargo fetch --locked --manifest-path backend/Cargo.toml

# Build actual backend (locked)
COPY backend/src ./backend/src
RUN cargo build --release --locked --manifest-path backend/Cargo.toml

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
