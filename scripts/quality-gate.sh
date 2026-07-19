#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

# shellcheck source=scripts/clippy-policy.sh
source "${SCRIPT_DIR}/clippy-policy.sh"

cd "${REPOSITORY_ROOT}"

echo "[quality] rustfmt"
cargo fmt --all -- --check

echo "[quality] default build has no host libpython dependency"
"${SCRIPT_DIR}/check-default-no-libpython.sh"

echo "[quality] clippy"
cargo clippy --workspace --all-targets --all-features --locked -- \
    "${CLIPPY_POLICY_ARGS[@]}"

echo "[quality] rustdoc"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked

echo "[quality] rustdoc tests"
RUSTDOCFLAGS="-D warnings" cargo test --workspace --all-features --doc --locked

echo "[quality] tests"
cargo test --workspace --all-targets --all-features --locked

echo "[quality] policy self-test"
"${SCRIPT_DIR}/quality-gate-self-test.sh"

echo "[quality] dependency audit"
"${SCRIPT_DIR}/dependency-audit.sh"

echo "[quality] passed"
