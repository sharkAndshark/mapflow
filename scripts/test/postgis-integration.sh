#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
COMPOSE_FILE="${ROOT_DIR}/docker-compose.postgis-test.yml"
SEED_FILE_IN_CONTAINER="/fixtures/seed.sql"

export POSTGIS_TEST_HOST="${POSTGIS_TEST_HOST:-127.0.0.1}"
export POSTGIS_TEST_PORT="${POSTGIS_TEST_PORT:-55432}"
export POSTGIS_TEST_DB="${POSTGIS_TEST_DB:-mapflow}"
export POSTGIS_TEST_USER="${POSTGIS_TEST_USER:-mapflow}"
export POSTGIS_TEST_PASSWORD="${POSTGIS_TEST_PASSWORD:-mapflow}"
export MAPFLOW_RUN_POSTGIS_TESTS=1
export APP_SECRET="${APP_SECRET:-postgis-integration-secret}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"

KEEP_FIXTURE="${KEEP_POSTGIS_FIXTURE:-false}"

cleanup() {
  if [[ "${KEEP_FIXTURE}" == "true" ]]; then
    echo "[postgis-integration] keep fixture enabled, skip docker compose down"
    return
  fi
  echo "[postgis-integration] stopping fixture"
  docker compose -f "${COMPOSE_FILE}" down -v >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "[postgis-integration] prebuilding postgis integration test binary"
cargo test --manifest-path "${ROOT_DIR}/backend/Cargo.toml" --test postgis_integration --no-run

echo "[postgis-integration] starting postgis fixture"
docker compose -f "${COMPOSE_FILE}" up -d postgis >/dev/null

echo "[postgis-integration] waiting for postgis readiness"
for _ in $(seq 1 60); do
  if docker compose -f "${COMPOSE_FILE}" exec -T postgis \
    pg_isready -U "${POSTGIS_TEST_USER}" -d "${POSTGIS_TEST_DB}" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! docker compose -f "${COMPOSE_FILE}" exec -T postgis \
  pg_isready -U "${POSTGIS_TEST_USER}" -d "${POSTGIS_TEST_DB}" >/dev/null 2>&1; then
  echo "[postgis-integration] postgis is not ready"
  docker compose -f "${COMPOSE_FILE}" logs postgis || true
  exit 1
fi

echo "[postgis-integration] seeding fixture data"
docker compose -f "${COMPOSE_FILE}" exec -T postgis \
  psql -v ON_ERROR_STOP=1 -U "${POSTGIS_TEST_USER}" -d "${POSTGIS_TEST_DB}" \
  -f "${SEED_FILE_IN_CONTAINER}" >/dev/null

echo "[postgis-integration] waiting for host port reachability ${POSTGIS_TEST_HOST}:${POSTGIS_TEST_PORT}"
for _ in $(seq 1 30); do
  if (echo >"/dev/tcp/${POSTGIS_TEST_HOST}/${POSTGIS_TEST_PORT}") >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! (echo >"/dev/tcp/${POSTGIS_TEST_HOST}/${POSTGIS_TEST_PORT}") >/dev/null 2>&1; then
  echo "[postgis-integration] host port is not reachable"
  docker compose -f "${COMPOSE_FILE}" logs postgis || true
  exit 1
fi

echo "[postgis-integration] running cargo test --test postgis_integration"
cargo test --manifest-path "${ROOT_DIR}/backend/Cargo.toml" --test postgis_integration -- --test-threads=1 --nocapture

echo "[postgis-integration] done"
