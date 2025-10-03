#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export RUST_BACKTRACE="${RUST_BACKTRACE:-full}"
DEFAULT_LOG_FILTER="automatafl_backend=trace,automatafl_logic=trace,tower_http=debug,axum::rejection=trace"
export RUST_LOG="${RUST_LOG:-${DEFAULT_LOG_FILTER}}"

cd "${REPO_ROOT}"

echo "Starting automatafl-backend with extended tracing"
echo "RUST_LOG=${RUST_LOG}"

time cargo run -p automatafl-backend "$@"
