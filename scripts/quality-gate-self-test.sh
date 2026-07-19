#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
FIXTURE_MANIFEST="${REPOSITORY_ROOT}/quality/fixtures/clippy-policy/Cargo.toml"
TEST_LOG_DIR="$(mktemp -d "${TMPDIR:-/tmp}/video-editor-quality.XXXXXX")"

# shellcheck source=scripts/clippy-policy.sh
source "${SCRIPT_DIR}/clippy-policy.sh"

cleanup() {
    case "${TEST_LOG_DIR}" in
        "${TMPDIR:-/tmp}"/video-editor-quality.*) rm -rf -- "${TEST_LOG_DIR}" ;;
        *) echo "refusing to remove unexpected path: ${TEST_LOG_DIR}" >&2 ;;
    esac
}
trap cleanup EXIT

ROOT_LOCKFILE="${REPOSITORY_ROOT}/Cargo.lock"

if [[ ! -f "${ROOT_LOCKFILE}" ]]; then
    echo "root Cargo.lock is required for reproducible quality gates" >&2
    exit 1
fi
if git -C "${REPOSITORY_ROOT}" check-ignore --quiet --no-index -- Cargo.lock; then
    echo "root Cargo.lock must not be ignored by git" >&2
    exit 1
fi

# This also catches a stale lockfile after workspace manifests change. On a
# fresh checkout, the existence check above guarantees Cargo.lock was tracked.
cargo metadata \
    --manifest-path "${REPOSITORY_ROOT}/Cargo.toml" \
    --locked \
    --no-deps \
    --format-version 1 \
    > /dev/null

run_fixture() {
    # This is an isolated, dependency-free policy fixture. Nested lockfiles are
    # deliberately ignored; the real workspace lock is validated above.
    cargo clippy \
        --manifest-path "${FIXTURE_MANIFEST}" \
        --all-targets \
        "$@" \
        -- \
        "${CLIPPY_POLICY_ARGS[@]}"
}

expect_policy_failure() {
    local feature="$1"
    local lint="$2"
    local lint_name="${lint#clippy::}"
    local lint_cli_name="${lint_name//_/-}"
    local log_file="${TEST_LOG_DIR}/${feature}.log"

    if run_fixture --features "${feature}" >"${log_file}" 2>&1; then
        echo "fixture ${feature} unexpectedly passed" >&2
        return 1
    fi
    if ! grep -Eq "clippy::(${lint_name}|${lint_cli_name})|#${lint_name}" "${log_file}"; then
        echo "fixture ${feature} failed for the wrong reason" >&2
        sed -n '1,160p' "${log_file}" >&2
        return 1
    fi
}

cargo fmt --manifest-path "${FIXTURE_MANIFEST}" -- --check
run_fixture

expect_policy_failure bad-allow-without-reason clippy::allow_attributes_without_reason
expect_policy_failure bad-case-sensitive-extension clippy::case_sensitive_file_extension_comparisons
expect_policy_failure bad-dbg clippy::dbg_macro
expect_policy_failure bad-ignored-result clippy::let_underscore_must_use
expect_policy_failure bad-large-stack-array clippy::large_stack_arrays
expect_policy_failure bad-large-value clippy::large_types_passed_by_value
expect_policy_failure bad-non-send-field clippy::non_send_fields_in_send_ty
expect_policy_failure bad-expect clippy::expect_used
expect_policy_failure bad-panic clippy::panic
expect_policy_failure bad-redundant-clone clippy::redundant_clone
expect_policy_failure bad-string-slice clippy::string_slice
expect_policy_failure bad-todo clippy::todo
expect_policy_failure bad-undocumented-unsafe clippy::undocumented_unsafe_blocks
expect_policy_failure bad-unimplemented clippy::unimplemented
expect_policy_failure bad-unreachable clippy::unreachable
expect_policy_failure bad-unused-result-ok clippy::unused_result_ok
expect_policy_failure bad-unwrap clippy::unwrap_used
expect_policy_failure bad-unwrap-in-result clippy::unwrap_in_result

VALID_EXCEPTION_FIXTURE="${REPOSITORY_ROOT}/quality/fixtures/dependency-audit/valid-exceptions.txt"
EXPIRED_EXCEPTION_FIXTURE="${REPOSITORY_ROOT}/quality/fixtures/dependency-audit/expired-exceptions.txt"
DUPLICATE_EXCEPTION_FIXTURE="${REPOSITORY_ROOT}/quality/fixtures/dependency-audit/duplicate-exceptions.txt"

QUALITY_AUDIT_VALIDATE_ONLY=1 \
    QUALITY_ADVISORY_EXCEPTION_FILE="${VALID_EXCEPTION_FIXTURE}" \
    "${SCRIPT_DIR}/dependency-audit.sh"

if QUALITY_AUDIT_VALIDATE_ONLY=1 \
    QUALITY_ADVISORY_EXCEPTION_FILE="${EXPIRED_EXCEPTION_FIXTURE}" \
    "${SCRIPT_DIR}/dependency-audit.sh" >"${TEST_LOG_DIR}/expired-advisory.log" 2>&1; then
    echo "expired advisory exception unexpectedly passed" >&2
    exit 1
fi
if ! grep -q "expired on" "${TEST_LOG_DIR}/expired-advisory.log"; then
    echo "expired advisory exception failed for the wrong reason" >&2
    cat "${TEST_LOG_DIR}/expired-advisory.log" >&2
    exit 1
fi

if QUALITY_AUDIT_VALIDATE_ONLY=1 \
    QUALITY_ADVISORY_EXCEPTION_FILE="${DUPLICATE_EXCEPTION_FIXTURE}" \
    "${SCRIPT_DIR}/dependency-audit.sh" >"${TEST_LOG_DIR}/duplicate-advisory.log" 2>&1; then
    echo "duplicate advisory exception unexpectedly passed" >&2
    exit 1
fi
if ! grep -q "duplicate advisory exception" "${TEST_LOG_DIR}/duplicate-advisory.log"; then
    echo "duplicate advisory exception failed for the wrong reason" >&2
    cat "${TEST_LOG_DIR}/duplicate-advisory.log" >&2
    exit 1
fi

echo "[quality] policy fixtures passed"
