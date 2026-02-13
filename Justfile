set shell := ["zsh", "-cu"]
set dotenv-load := true

PORT := env_var_or_default("PORT", "3000")
VITE_PORT := env_var_or_default("VITE_PORT", "5173")
DB_PATH := env_var_or_default("DB_PATH", "./data/mapflow.duckdb")
UPLOAD_DIR := env_var_or_default("UPLOAD_DIR", "./uploads")

# Default: local dev (backend + frontend)
start: dev

# Run both backend and frontend in parallel (auto-selecting ports)
dev:
  ./scripts/dev.sh

# Dev backend only
dev-backend:
  PORT={{PORT}} cargo run --manifest-path backend/Cargo.toml

# Dev frontend only
dev-frontend:
  PORT={{PORT}} VITE_PORT={{VITE_PORT}} npm --prefix frontend run dev

# Install dependencies (backend is automatic, frontend needs install)
install:
  npm --prefix frontend install

# Check code quality
check:
  bash scripts/ci/check_spatial_extension_version.sh
  cargo fmt --manifest-path backend/Cargo.toml -- --check
  cargo clippy --manifest-path backend/Cargo.toml -- -D warnings
  npm --prefix frontend run format:check

# Fix code quality issues
fix:
  cargo fmt --manifest-path backend/Cargo.toml
  cargo clippy --manifest-path backend/Cargo.toml --fix --allow-dirty --allow-staged

# Build local artifacts (no docker)
build:
  npm --prefix frontend run build
  cargo build --release --manifest-path backend/Cargo.toml

# --- Docker Operations (Explicit) ---

# Start container (no rebuild by default)
docker-up:
  docker compose up -d
  @echo "MapFlow started at http://localhost:3000"

# Start container with forced rebuild
docker-up-build:
  docker compose up -d --build
  @echo "MapFlow rebuilt and started at http://localhost:3000"

# Stop container
docker-down:
  docker compose down

# Show container logs
docker-logs:
  docker compose logs -f

# Check container status
docker-ps:
  docker compose ps

# Enter container shell
docker-shell:
  docker compose exec mapflow /bin/bash

# --- Testing ---

# Run backend unit/integration tests
test-backend:
  cargo test --manifest-path backend/Cargo.toml

# Run frontend unit tests
test-frontend-unit:
  npm --prefix frontend run test:unit

# Run frontend e2e tests (requires build first or running server)
test-e2e:
  npm --prefix frontend run test:e2e

# Run all tests
test: test-backend test-frontend-unit test-e2e
  @echo "All tests complete"

# --- Cleanup ---

# Remove DuckDB file(s) (respects DB_PATH)
clean-db:
  rm -f "{{DB_PATH}}" "{{DB_PATH}}.wal" "{{DB_PATH}}.tmp" "{{DB_PATH}}.shm" "{{DB_PATH}}-wal" "{{DB_PATH}}-shm"

# Remove uploads directory (respects UPLOAD_DIR)
clean-uploads:
  rm -rf "{{UPLOAD_DIR}}"

# Clean local dev state (db + uploads)
clean: clean-db clean-uploads

# Remove frontend build output
clean-web:
  rm -rf "frontend/dist"

# Remove local test artifacts (Playwright output)
clean-test-artifacts:
  rm -rf "frontend/playwright-report" "frontend/test-results" "playwright-report" "test-results"

# Remove local tmp state (keeps tmp/osm_samples and other ad-hoc scratch)
clean-tmp:
  rm -rf tmp/worker-* tmp/test-* tmp/release-* tmp/release-uploads tmp/test-uploads

# Clean typical local dev state (db + uploads + tmp + test artifacts + built web)
clean-all: clean clean-web clean-test-artifacts clean-tmp

# Optional: remove Node install caches (slow to rebuild)
clean-node:
  rm -rf "node_modules" "frontend/node_modules"

# Optional: remove Rust build cache (slow to rebuild)
clean-target:
  cargo clean

# --- Dependency Management ---

# Check for outdated dependencies (run daily)
outdated:
  @echo "=== Frontend outdated ==="
  npm outdated --prefix frontend || true
  @echo "\n=== Backend outdated ==="
  cargo update --dry-run --manifest-path backend/Cargo.toml

# Install frontend dependencies (first time or CI)
install-frontend:
  cd frontend && npm install

# Update frontend dependencies (patch/minor versions only)
update-frontend:
  @echo "Updating frontend dependencies..."
  cd frontend && npm update
  @echo "✅ Frontend dependencies updated"
  @echo "Next steps:"
  @echo "  1. Run tests: just test-frontend-unit"
  @echo "  2. Build project: just build"

# Update backend dependencies
update-backend:
  @echo "Updating backend dependencies..."
  cargo update --manifest-path backend/Cargo.toml
  @echo "✅ Backend dependencies updated"
  @echo "Next: Run tests to verify"
  @echo "  just test-backend"

# Update all dependencies
update-all: update-frontend update-backend
  @echo "\n✅ All dependencies updated"
  @echo "Run full test suite: just test"

# Update specific frontend package
update-frontend-pkg PACKAGE:
  cd frontend && npm update {{PACKAGE}}
  @echo "✅ Updated {{PACKAGE}}"
  @echo "Run tests to verify: just test-frontend-unit"

# Update specific backend package
update-backend-pkg PACKAGE:
  cargo update --manifest-path backend/Cargo.toml --package {{PACKAGE}}
  @echo "✅ Updated {{PACKAGE}}"
  @echo "Run tests to verify: just test-backend"
