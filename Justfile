set shell := ["zsh", "-cu"]
set dotenv-load := true

PORT := env_var_or_default("PORT", "3000")
VITE_PORT := env_var_or_default("VITE_PORT", "5173")

# Default: local dev (backend + frontend)
start: dev

# Run both backend and frontend in parallel (auto-selecting ports)
dev:
  @echo "Starting dev environment..."
  @echo "---------------------------------------------------"
  @echo "Backend will run at: http://localhost:3000"
  @echo "Frontend will run at: http://localhost:5173"
  @echo "---------------------------------------------------"
  PORT=3000 cargo run --manifest-path backend/Cargo.toml & PID_BACKEND=$!; \
  PORT=3000 VITE_PORT=5173 npm --prefix frontend run dev -- --port 5173 --strictPort & PID_FRONTEND=$!; \
  trap "kill $PID_BACKEND $PID_FRONTEND" INT TERM EXIT; \
  wait

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
  cargo fmt --manifest-path backend/Cargo.toml -- --check
  cargo clippy --manifest-path backend/Cargo.toml -- -D warnings

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

# Run frontend e2e tests (requires build first or running server)
test-e2e:
  npm --prefix frontend run test:e2e

# Run all tests
test: test-backend test-e2e
  @echo "All tests complete"
