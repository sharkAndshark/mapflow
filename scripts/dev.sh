#!/usr/bin/env bash
set -euo pipefail

# Load nvm to ensure correct Node.js version
export NVM_DIR="$HOME/.nvm"
if [ -s "$NVM_DIR/nvm.sh" ]; then
    \. "$NVM_DIR/nvm.sh"
    
    # Use .nvmrc if present, otherwise rely on user's active version
    if [ -f ".nvmrc" ]; then
        if ! nvm use >/dev/null 2>&1; then
            NVMRC_VERSION=$(cat .nvmrc)
            echo "[dev.sh] ERROR: Failed to activate Node.js $NVMRC_VERSION from .nvmrc" >&2
            echo "[dev.sh] ERROR: Please install: nvm install $NVMRC_VERSION" >&2
            exit 1
        fi
    fi
else
    echo "[dev.sh] ERROR: NVM not found at \$NVM_DIR/nvm.sh" >&2
    echo "[dev.sh] ERROR: Please install NVM or ensure Node.js 20.19+/22.12+ is active" >&2
    exit 1
fi

# Verify Node version meets Vite 7.x requirements (^20.19.0 || >=22.12.0)
NODE_VERSION=$(node -v)
NODE_VERSION_NUMBERS=$(echo "$NODE_VERSION" | cut -d'v' -f2 | cut -d'.' -f1,2)
NODE_MAJOR=$(echo "$NODE_VERSION_NUMBERS" | cut -d'.' -f1)
NODE_MINOR=$(echo "$NODE_VERSION_NUMBERS" | cut -d'.' -f2)

# Validate version parsing
if [ -z "$NODE_MAJOR" ] || [ -z "$NODE_MINOR" ]; then
    echo "[dev.sh] ERROR: Unable to parse Node.js version from: $NODE_VERSION" >&2
    exit 1
fi

# Allow v20.19.0+, v22.12.0+, v23.x+
if { [ "$NODE_MAJOR" -eq 20 ] && [ "$NODE_MINOR" -ge 19 ]; } || \
   { [ "$NODE_MAJOR" -eq 22 ] && [ "$NODE_MINOR" -ge 12 ]; } || \
   [ "$NODE_MAJOR" -gt 22 ]; then
    : # Version is acceptable
else
    echo "[dev.sh] ERROR: Node.js $NODE_VERSION does not meet requirement (v20.19.0+ or v22.12.0+)" >&2
    exit 1
fi

# -----------------------------------------------------------------------------
# Configuration
# -----------------------------------------------------------------------------
PORT="${PORT:-3000}"
VITE_PORT="${VITE_PORT:-5173}"

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

log() { echo -e "${GREEN}[dev.sh]${NC} $1"; }
warn() { echo -e "${YELLOW}[dev.sh]${NC} $1"; }
err()  { echo -e "${RED}[dev.sh]${NC} $1"; }

# -----------------------------------------------------------------------------
# Cleanup Logic
# -----------------------------------------------------------------------------
BACKEND_PID=""
FRONTEND_PID=""
TAIL_PID=""
BACKEND_STDERR=""

kill_pid_and_children() {
    local pid="$1"
    local name="$2"

    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        warn "Stopping $name (PID: $pid)..."
        kill -TERM "$pid" 2>/dev/null || true
        
        # Give it a moment to shutdown gracefully
        local count=0
        while kill -0 "$pid" 2>/dev/null && [ "$count" -lt 20 ]; do
            sleep 0.1
            count=$((count + 1))
        done

        # Force kill if still running
        if kill -0 "$pid" 2>/dev/null; then
            warn "$name did not exit gracefully, force killing..."
            kill -KILL "$pid" 2>/dev/null || true
        fi
    fi
}

cleanup() {
    local exit_code="${1:-0}"
    trap - INT TERM EXIT

    echo ""
    log "Shutting down..."

    kill_pid_and_children "$FRONTEND_PID" "Frontend"
    kill_pid_and_children "$BACKEND_PID" "Backend"
    
    if [ -n "$TAIL_PID" ] && kill -0 "$TAIL_PID" 2>/dev/null; then
        kill -TERM "$TAIL_PID" 2>/dev/null || true
    fi

    rm -f "$BACKEND_STDERR" 2>/dev/null || true

    exit "$exit_code"
}

trap 'cleanup 130' INT TERM
trap 'cleanup 0' EXIT

# -----------------------------------------------------------------------------
# Pre-flight Check & Port Cleanup
# -----------------------------------------------------------------------------
log "Checking ports..."

kill_port_owner() {
    local port="$1"
    local pids
    pids=$(lsof -ti:"$port" 2>/dev/null || true)
    
    if [ -n "$pids" ]; then
        warn "Port $port is in use by PID(s): $pids. Killing..."
        echo "$pids" | xargs kill -TERM 2>/dev/null || true
        sleep 1
        pids=$(lsof -ti:"$port" 2>/dev/null || true)
        if [ -n "$pids" ]; then
            echo "$pids" | xargs kill -KILL 2>/dev/null || true
        fi
    fi
}

# Only kill frontend port; backend will auto-select an available port
kill_port_owner "$VITE_PORT"

# -----------------------------------------------------------------------------
# Build Backend
# -----------------------------------------------------------------------------
log "Building backend..."
# Using --manifest-path ensures we build correctly from repo root
cargo build --manifest-path backend/Cargo.toml

if [ $? -ne 0 ]; then
    err "Backend build failed."
    # Exit without triggering full cleanup (since nothing started)
    trap - INT TERM EXIT
    exit 1
fi

# -----------------------------------------------------------------------------
# Start Backend
# -----------------------------------------------------------------------------
# NOTE: In a workspace, the default target dir is ./target (repo root), not ./backend/target
BINARY_PATH="./target/debug/backend"

if [ ! -f "$BINARY_PATH" ]; then
    err "Binary not found at $BINARY_PATH. Build might have failed silently or output path is different."
    # Fallback check for common alternative
    if [ -f "./backend/target/debug/backend" ]; then
        warn "Found binary at ./backend/target/debug/backend instead."
        BINARY_PATH="./backend/target/debug/backend"
    else
        exit 1
    fi
fi

# Create a temp file for backend stderr (to capture PORT=xxx output)
BACKEND_STDERR=$(mktemp)

log "Starting backend (preferred port: $PORT)..."
PORT="$PORT" "$BINARY_PATH" 2>"$BACKEND_STDERR" &
BACKEND_PID=$!
log "Backend PID: $BACKEND_PID"

# -----------------------------------------------------------------------------
# Wait for Backend Port
# -----------------------------------------------------------------------------
ACTUAL_PORT=""
max_wait=60
count=0

while [ "$count" -lt "$max_wait" ]; do
    if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
        err "Backend process died unexpectedly."
        cat "$BACKEND_STDERR" >&2
        exit 1
    fi

    # Check for PORT=xxx in stderr
    ACTUAL_PORT=$(grep -m1 "^PORT=" "$BACKEND_STDERR" 2>/dev/null | cut -d= -f2 || true)
    if [ -n "$ACTUAL_PORT" ]; then
        break
    fi

    sleep 0.5
    count=$((count + 1))
done

if [ -z "$ACTUAL_PORT" ]; then
    err "Backend did not report port within 30s."
    cat "$BACKEND_STDERR" >&2
    exit 1
fi

if [ "$ACTUAL_PORT" != "$PORT" ]; then
    warn "Port $PORT in use, backend using $ACTUAL_PORT instead"
fi
log "Backend is ready on port $ACTUAL_PORT!"

# Tail backend logs to console (skip the PORT= line we already processed)
tail -f "$BACKEND_STDERR" 2>&1 | grep --line-buffered -v "^PORT=" &
TAIL_PID=$!

# -----------------------------------------------------------------------------
# Wait for Backend API Ready
# -----------------------------------------------------------------------------
log "Waiting for backend API..."
max_retries=60
count=0
api_ready=0

while [ "$count" -lt "$max_retries" ]; do
    if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
        err "Backend process died unexpectedly."
        cat "$BACKEND_STDERR" >&2
        exit 1
    fi

    if curl -s "http://127.0.0.1:$ACTUAL_PORT/api/files" >/dev/null 2>&1; then
        api_ready=1
        break
    fi

    sleep 0.5
    count=$((count + 1))
done

if [ "$api_ready" -eq 0 ]; then
    err "Backend API failed to respond in 30s."
    grep -v "^PORT=" "$BACKEND_STDERR" >&2 || true
    exit 1
fi

# -----------------------------------------------------------------------------
# Start Frontend
# -----------------------------------------------------------------------------
log "Starting frontend on port $VITE_PORT..."
PORT="$ACTUAL_PORT" VITE_PORT="$VITE_PORT" npm --prefix frontend run dev -- --port "$VITE_PORT" --strictPort &
FRONTEND_PID=$!
log "Frontend PID: $FRONTEND_PID"

# Print summary
echo ""
log "=========================================="
log "  Frontend: http://localhost:$VITE_PORT"
log "  Backend:  http://localhost:$ACTUAL_PORT"
log "=========================================="
echo ""

# -----------------------------------------------------------------------------
# Watch Loop (Bash 3.2 Compatible)
# -----------------------------------------------------------------------------
log "Dev environment running. Press Ctrl+C to stop."

while kill -0 "$BACKEND_PID" 2>/dev/null && kill -0 "$FRONTEND_PID" 2>/dev/null; do
    sleep 1
done

err "One of the services exited unexpectedly."
exit 1
