# Developer Documentation

This document serves as the entry point for developers and AI agents working on MapFlow. It defines the development environment, system behaviors, and testing protocols.

## Quick Links
- [System Behaviors & Contracts](./behaviors.md): API definitions, storage conventions, and technical details.
- [Testing & Quality Assurance](./testing.md): Behavior checklist, E2E test setup, and CI gates.

## Repository Map
- `/backend`: Rust Axum server (API & data processing).
- `/frontend`: React + Vite application.
- `/docs/dev`: Developer documentation and specs.
- `/tests`: Integration/E2E tests (if applicable outside frontend).
- `AGENTS.md`: Operational constraints and long-term memory for AI agents.

## Local Development

### Prerequisites
- Rust (latest stable)
- Node.js & npm
- DuckDB (optional, binary included in build or system dependency depending on config)
- `just` (optional task runner)

### Environment Configuration (.env)

Copy the template: `cp .env.example .env`

**Backend Variables:**
- `PORT`: Service port (default: `3000`)
- `UPLOAD_MAX_SIZE_MB`: Max upload size in MB (default: `200`)
- `UPLOAD_DIR`: Storage directory (default: `./uploads`)
- `DB_PATH`: DuckDB file path (default: `./data/mapflow.duckdb`)

**Frontend Variables:**
- `VITE_PORT`: Dev server port (default: `5173`)

**Test Variables:**
- `TEST_PORT`: E2E test port (default: `3000`)

### Running the Project

**Using `just` (Recommended):**
- `just start`: Run both backend and frontend.
- `just check`: Run code quality checks (fmt + clippy).
- `just test`: Run all tests.

**Manual Start:**
1. **Backend:** `cargo run --manifest-path backend/Cargo.toml`
2. **Frontend:** `cd frontend && npm run dev`

### Code Quality & Git Hooks

The repository enforces strict code quality standards.

**Install Hooks:**
```bash
./scripts/install-hooks.sh
```

**Checks run on commit:**
- Rust: `cargo fmt --check` and `cargo clippy -D warnings`
- Frontend: `npm --prefix frontend run format:check` (Biome)
