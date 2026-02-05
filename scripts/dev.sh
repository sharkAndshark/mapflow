#!/usr/bin/env bash
set -uo pipefail

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
        # Split by newline and kill each
        echo "$pids" | xargs kill -TERM 2>/dev/null || true
        sleep 1
        # Force kill survivors
        pids=$(lsof -ti:"$port" 2>/dev/null || true)
        if [ -n "$pids" ]; then
            echo "$pids" | xargs kill -KILL 2>/dev/null || true
        fi
    fi
}

kill_port_owner "$PORT"
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

log "Starting backend on port $PORT..."
PORT="$PORT" "$BINARY_PATH" &
BACKEND_PID=$!
log "Backend PID: $BACKEND_PID"

# -----------------------------------------------------------------------------
# Wait for Backend Ready
# -----------------------------------------------------------------------------
log "Waiting for backend to be ready..."
max_retries=60 # 30 seconds
count=0
backend_ready=0

while [ "$count" -lt "$max_retries" ]; do
    # Check if process died early
    if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
        err "Backend process died unexpectedly."
        exit 1
    fi

    if curl -s "http://127.0.0.1:$PORT/api/files" >/dev/null; then
        backend_ready=1
        break
    fi

    sleep 0.5
    count=$((count + 1))
done

if [ "$backend_ready" -eq 0 ]; then
    err "Backend failed to start in 30s."
    exit 1
fi

log "Backend is ready!"

# -----------------------------------------------------------------------------
# Start Frontend
# -----------------------------------------------------------------------------
log "Starting frontend on port $VITE_PORT..."
PORT="$PORT" VITE_PORT="$VITE_PORT" npm --prefix frontend run dev -- --port "$VITE_PORT" --strictPort &
FRONTEND_PID=$!
log "Frontend PID: $FRONTEND_PID"

# -----------------------------------------------------------------------------
# Watch Loop (Bash 3.2 Compatible)
# -----------------------------------------------------------------------------
log "Dev environment running. Press Ctrl+C to stop."

while kill -0 "$BACKEND_PID" 2>/dev/null && kill -0 "$FRONTEND_PID" 2>/dev/null; do
    sleep 1
done

err "One of the services exited unexpectedly."
exit 1
