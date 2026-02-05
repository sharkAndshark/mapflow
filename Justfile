set shell := ["zsh", "-cu"]

# Build and start the container in detached mode
start:
  docker compose up -d --build
  @echo "MapFlow started at http://localhost:3000"

# Stop the container
stop:
  docker compose down

# Show logs
logs:
  docker compose logs -f

# Check status
ps:
  docker compose ps

# Enter container shell
shell:
  docker compose exec mapflow /bin/bash

# Build frontend and backend locally (for non-docker runs)
build:
  cd frontend && npm run build
  cargo build --manifest-path backend/Cargo.toml

# Run backend tests
backend-test:
  cargo test --manifest-path backend/Cargo.toml

# Run frontend e2e tests (builds first)
e2e:
  cd frontend && npm run test:e2e

# Full test suite
test: backend-test e2e
  echo "All tests complete"
