#!/usr/bin/env bash
# full local gate, mirroring .github/workflows/ci.yml. the pre-push hook runs
# this before anything reaches origin. the DB-gated kernel tests need
# DATABASE_URL (set in the devcontainer; they self-skip without it).

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

echo "== fmt + clippy"
cargo fmt --all --check
cargo clippy --workspace --exclude ttrpg-dice-engine --all-targets -- -D warnings

echo "== plugin components"
./plugins/build.sh

echo "== rust tests"
cargo test --workspace

echo "== web build + tests"
cd web
[ -d node_modules/.bin ] || npm ci
npm run build
npm test

echo "== all gates green"
