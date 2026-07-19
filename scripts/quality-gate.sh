#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

# shellcheck source=scripts/clippy-policy.sh
source "${SCRIPT_DIR}/clippy-policy.sh"

cd "${REPOSITORY_ROOT}"

echo "[quality] shell syntax and policy self-test"
bash -n "${SCRIPT_DIR}"/*.sh
"${SCRIPT_DIR}/quality-gate-self-test.sh"

echo "[quality] rustfmt"
cargo fmt --all -- --check

echo "[quality] default build has no host libpython dependency"
"${SCRIPT_DIR}/check-default-no-libpython.sh"

echo "[quality] cargo check (default features)"
cargo check --workspace --all-targets --locked

# `--all-features` does not compile branches guarded by
# `cfg(not(feature = "..."))`. Keep this exact production/default pass even
# though the maximal-feature, all-target pass below is broader in other ways.
echo "[quality] clippy (production targets, default features)"
cargo clippy --workspace --lib --bins --locked -- \
    "${CLIPPY_POLICY_ARGS[@]}"

# The library's default enables the GL renderer, so neither workspace/default
# nor workspace/all-features sees the CPU fallback.
echo "[quality] clippy (library CPU fallback)"
cargo clippy -p library --lib --no-default-features --locked -- \
    "${CLIPPY_POLICY_ARGS[@]}"

echo "[quality] clippy (all targets, all features)"
cargo clippy --workspace --all-targets --all-features --locked -- \
    "${CLIPPY_POLICY_ARGS[@]}"

echo "[quality] rustdoc"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked

echo "[quality] rustdoc tests"
RUSTDOCFLAGS="-D warnings" cargo test --workspace --all-features --doc --locked

echo "[quality] tests (default features)"
cargo test --workspace --all-targets --locked

echo "[quality] tests (all features)"
cargo test --workspace --all-targets --all-features --locked

echo "[quality] dependency audit"
env \
    -u QUALITY_ADVISORY_EXCEPTION_FILE \
    -u QUALITY_AUDIT_VALIDATE_ONLY \
    -u QUALITY_TOOL_ROOT \
    "${SCRIPT_DIR}/dependency-audit.sh"

echo "[quality] passed"
