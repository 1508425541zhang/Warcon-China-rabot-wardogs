#!/usr/bin/env bash
# Native backend regression against an isolated local PostgreSQL server.
# Requires Rust, Node, Bun (build/test tool), OpenSSL development libraries and psql.
# CI_DB is a PostgreSQL URL without a database name. No production .env is loaded.
set -euo pipefail
cd "$(dirname "$0")/.."
export TEST_DATABASE_URL="${CI_DB:-postgres://warcon:warcon@127.0.0.1:5434}/postgres"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.98.1}"

bun install --frozen-lockfile
cargo fmt --manifest-path backend/Cargo.toml --check
cargo test --manifest-path backend/Cargo.toml --locked -- --include-ignored
cargo build --manifest-path backend/Cargo.toml --locked --bins
bun --no-env-file run test
bun --no-env-file run check
node node_modules/vite/bin/vite.js build
SMOKE_PSQL=psql SMOKE_DATABASE_URL="$TEST_DATABASE_URL" \
  SMOKE_BIN_DIR=backend/target/debug \
  SMOKE_MODEL_ARTIFACTS="${SMOKE_MODEL_ARTIFACTS:-services/integrity-model-bun/artifacts-30m}" \
  node backend/tools/local-stack-smoke.mjs

if [ "${1:-}" = "--docker" ]; then
  docker build -f Dockerfile.native -t warcon-cn-native:ci .
  docker build -t warcon-cn-frontend:ci .
fi
printf 'Native regression passed. External provider and real game connections require separate testing.\n'
