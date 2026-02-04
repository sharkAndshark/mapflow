set shell := ["zsh", "-cu"]

# Start backend and frontend dev servers in two terminals
# Usage: just dev

dev:
  echo "Run in two terminals:"
  echo "  1) cargo run --manifest-path backend/Cargo.toml"
  echo "  2) cd frontend && npm run dev"

# Build frontend and start backend + frontend in background
start:
  mkdir -p .just
  cd frontend && npm run build
  nohup cargo run --manifest-path backend/Cargo.toml > .just/backend.log 2>&1 & echo $! > .just/backend.pid
  nohup sh -c "cd frontend && npm run dev" > .just/frontend.log 2>&1 & echo $! > .just/frontend.pid
  echo "Started backend PID: $(cat .just/backend.pid)"
  echo "Started frontend PID: $(cat .just/frontend.pid)"

# Stop background services started by `just start`
stop:
  if [ -f .just/backend.pid ]; then kill $(cat .just/backend.pid) || true; rm -f .just/backend.pid; fi
  if [ -f .just/frontend.pid ]; then kill $(cat .just/frontend.pid) || true; rm -f .just/frontend.pid; fi
  echo "Stopped background services"

# Build frontend and backend for production-like run
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
