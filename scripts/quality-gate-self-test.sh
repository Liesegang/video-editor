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
RUNTIME_PROPERTY_LOCKFILE="${REPOSITORY_ROOT}/plugins/random_property/Cargo.lock"
QUALITY_GATE="${SCRIPT_DIR}/quality-gate.sh"
RUST_FILE_SIZE_GATE="${SCRIPT_DIR}/check-rust-file-size.sh"
RUST_FILE_SIZE_RATCHET="${SCRIPT_DIR}/check-rust-file-size-ratchet.sh"

if [[ ! -f "${ROOT_LOCKFILE}" ]]; then
    echo "root Cargo.lock is required for reproducible quality gates" >&2
    exit 1
fi
if git -C "${REPOSITORY_ROOT}" check-ignore --quiet --no-index -- Cargo.lock; then
    echo "root Cargo.lock must not be ignored by git" >&2
    exit 1
fi
if ! git -C "${REPOSITORY_ROOT}" ls-files --error-unmatch -- Cargo.lock > /dev/null; then
    echo "root Cargo.lock must be tracked by git" >&2
    exit 1
fi
if [[ ! -f "${RUNTIME_PROPERTY_LOCKFILE}" ]]; then
    echo "standalone runtime property plugin Cargo.lock is required" >&2
    exit 1
fi
if git -C "${REPOSITORY_ROOT}" check-ignore --quiet --no-index -- \
    plugins/random_property/Cargo.lock; then
    echo "standalone runtime property plugin Cargo.lock must not be ignored" >&2
    exit 1
fi
if ! git -C "${REPOSITORY_ROOT}" ls-files --error-unmatch -- \
    plugins/random_property/Cargo.lock > /dev/null; then
    echo "standalone runtime property plugin Cargo.lock must be tracked by git" >&2
    exit 1
fi

require_gate_command() {
    local command="$1"
    if ! awk '
        {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            if (line !~ /^#/) print line
        }
    ' "${QUALITY_GATE}" | grep -Fqx -- "${command}"; then
        echo "quality gate is missing required command: ${command}" >&2
        exit 1
    fi
}

# These checks make deletion of an expensive stage fail closed. Keep the
# asserted command prefixes on one line in quality-gate.sh.
require_gate_command 'set -euo pipefail'
require_gate_command 'bash -n "${SCRIPT_DIR}"/*.sh'
require_gate_command 'cargo fmt --all -- --check'
require_gate_command 'cargo fmt --manifest-path "${RUNTIME_PROPERTY_PLUGIN_MANIFEST}" -- --check'
require_gate_command 'source "${SCRIPT_DIR}/managed-python-env.sh"'
require_gate_command '"${SCRIPT_DIR}/check-managed-cpython.sh"'
require_gate_command 'cargo check --workspace --all-targets --locked'
require_gate_command 'cargo check --manifest-path "${RUNTIME_PROPERTY_PLUGIN_MANIFEST}" --all-targets --locked'
require_gate_command 'cargo clippy --workspace --lib --bins --locked -- \'
require_gate_command 'cargo clippy -p library --lib --no-default-features --locked -- \'
require_gate_command 'cargo clippy --workspace --all-targets --all-features --locked -- \'
require_gate_command 'cargo clippy --manifest-path "${RUNTIME_PROPERTY_PLUGIN_MANIFEST}" --all-targets --locked -- \'
require_gate_command 'cargo test --workspace --all-targets --locked'
require_gate_command 'cargo test --manifest-path "${RUNTIME_PROPERTY_PLUGIN_MANIFEST}" --all-targets --locked'
require_gate_command 'cargo test --workspace --all-targets --all-features --locked'
require_gate_command 'RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked'
require_gate_command 'RUSTDOCFLAGS="-D warnings" cargo test --workspace --all-features --doc --locked'
require_gate_command '"${SCRIPT_DIR}/quality-gate-self-test.sh"'
require_gate_command '"${SCRIPT_DIR}/check-rust-file-size-ratchet.sh"'
require_gate_command '-u QUALITY_ADVISORY_EXCEPTION_FILE \'
require_gate_command '-u QUALITY_AUDIT_VALIDATE_ONLY \'
require_gate_command '-u QUALITY_TOOL_ROOT \'
require_gate_command '"${SCRIPT_DIR}/dependency-audit.sh"'

RUST_SIZE_FIXTURE="${TEST_LOG_DIR}/rust-file-size"
mkdir -p "${RUST_SIZE_FIXTURE}"
awk 'BEGIN { for (line = 1; line <= 3; line += 1) print "// line" }' \
    > "${RUST_SIZE_FIXTURE}/boundary.rs"
"${RUST_FILE_SIZE_GATE}" --root "${RUST_SIZE_FIXTURE}" --max-lines 3 \
    > "${TEST_LOG_DIR}/rust-file-size-pass.log"

awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_SIZE_FIXTURE}/too-large.rs"
if "${RUST_FILE_SIZE_GATE}" \
    --root "${RUST_SIZE_FIXTURE}" \
    --max-lines 3 \
    > "${TEST_LOG_DIR}/rust-file-size-fail.log" 2>&1; then
    echo "oversized Rust file unexpectedly passed" >&2
    exit 1
fi
if ! grep -Fq 'too-large.rs: 4 lines (limit 3)' \
    "${TEST_LOG_DIR}/rust-file-size-fail.log"; then
    echo "Rust file size fixture failed for the wrong reason" >&2
    cat "${TEST_LOG_DIR}/rust-file-size-fail.log" >&2
    exit 1
fi
rm -- "${RUST_SIZE_FIXTURE}/too-large.rs"

awk 'BEGIN { for (line = 1; line <= 1001; line += 1) print "// line" }' \
    > "${RUST_SIZE_FIXTURE}/production-limit.rs"
if "${RUST_FILE_SIZE_GATE}" --root "${RUST_SIZE_FIXTURE}" \
    > "${TEST_LOG_DIR}/rust-file-size-default.log" 2>&1; then
    echo "default Rust file size limit unexpectedly accepted 1001 lines" >&2
    exit 1
fi
if ! grep -Fq 'production-limit.rs: 1001 lines (limit 1000)' \
    "${TEST_LOG_DIR}/rust-file-size-default.log"; then
    echo "default Rust file size fixture failed for the wrong reason" >&2
    cat "${TEST_LOG_DIR}/rust-file-size-default.log" >&2
    exit 1
fi

RUST_RATCHET_FIXTURE="${TEST_LOG_DIR}/rust-file-size-ratchet"
mkdir -p "${RUST_RATCHET_FIXTURE}"
git -C "${RUST_RATCHET_FIXTURE}" init -q
git -C "${RUST_RATCHET_FIXTURE}" config user.email quality@example.invalid
git -C "${RUST_RATCHET_FIXTURE}" config user.name 'Quality Gate'
awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/existing-oversized.rs"
awk 'BEGIN { for (line = 1; line <= 3; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/boundary.rs"
: > "${RUST_RATCHET_FIXTURE}/empty.rs"
ODD_RUST_FILE=$'line\nbreak.rs'
awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/${ODD_RUST_FILE}"
awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/space name.rs"
awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/-dash.rs"
git -C "${RUST_RATCHET_FIXTURE}" add -- '*.rs'
git -C "${RUST_RATCHET_FIXTURE}" commit -qm baseline
git -C "${RUST_RATCHET_FIXTURE}" branch -m fixture-head
RUST_RATCHET_BASELINE="$(git -C "${RUST_RATCHET_FIXTURE}" rev-parse HEAD)"

# An unavailable implicit integration base must fail closed rather than
# silently comparing the branch with itself.
if "${RUST_FILE_SIZE_RATCHET}" --root "${RUST_RATCHET_FIXTURE}" \
    > "${TEST_LOG_DIR}/rust-file-size-ratchet-no-baseline.log" 2>&1; then
    echo "Rust file size ratchet unexpectedly passed without a baseline" >&2
    exit 1
fi
if ! grep -Fq 'Rust file size baseline is required' \
    "${TEST_LOG_DIR}/rust-file-size-ratchet-no-baseline.log"; then
    echo "missing Rust file size baseline failed for the wrong reason" >&2
    cat "${TEST_LOG_DIR}/rust-file-size-ratchet-no-baseline.log" >&2
    exit 1
fi

# Comparing with an unrelated or newer commit can hide branch regressions, so
# only an actual ancestor may serve as the integration baseline.
git -C "${RUST_RATCHET_FIXTURE}" checkout -qb unrelated
echo '// unrelated history' > "${RUST_RATCHET_FIXTURE}/unrelated.rs"
git -C "${RUST_RATCHET_FIXTURE}" add -- unrelated.rs
git -C "${RUST_RATCHET_FIXTURE}" commit -qm unrelated
RUST_RATCHET_UNRELATED="$(git -C "${RUST_RATCHET_FIXTURE}" rev-parse HEAD)"
git -C "${RUST_RATCHET_FIXTURE}" checkout -q --detach "${RUST_RATCHET_BASELINE}"
if "${RUST_FILE_SIZE_RATCHET}" --root "${RUST_RATCHET_FIXTURE}" \
    --baseline-ref "${RUST_RATCHET_UNRELATED}" --max-lines 3 \
    > "${TEST_LOG_DIR}/rust-file-size-ratchet-unrelated.log" 2>&1; then
    echo "Rust file size ratchet unexpectedly accepted a non-ancestor baseline" >&2
    exit 1
fi
if ! grep -Fq 'Rust file size baseline must be an ancestor of HEAD' \
    "${TEST_LOG_DIR}/rust-file-size-ratchet-unrelated.log"; then
    echo "non-ancestor Rust file size baseline failed for the wrong reason" >&2
    cat "${TEST_LOG_DIR}/rust-file-size-ratchet-unrelated.log" >&2
    exit 1
fi

# Existing debt may shrink or remain unchanged.
awk 'BEGIN { for (line = 1; line <= 3; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/existing-oversized.rs"
"${RUST_FILE_SIZE_RATCHET}" --root "${RUST_RATCHET_FIXTURE}" \
    --baseline-ref "${RUST_RATCHET_BASELINE}" --max-lines 3 \
    > "${TEST_LOG_DIR}/rust-file-size-ratchet-pass.log"

expect_ratchet_failure() {
    local expected="$1"
    local log_file="${TEST_LOG_DIR}/rust-file-size-ratchet-fail.log"
    if "${RUST_FILE_SIZE_RATCHET}" --root "${RUST_RATCHET_FIXTURE}" \
        --baseline-ref "${RUST_RATCHET_BASELINE}" --max-lines 3 \
        >"${log_file}" 2>&1; then
        echo "Rust file size regression unexpectedly passed" >&2
        exit 1
    fi
    if ! grep -Fq "${expected}" "${log_file}"; then
        echo "Rust file size ratchet failed for the wrong reason" >&2
        cat "${log_file}" >&2
        exit 1
    fi
}

awk 'BEGIN { for (line = 1; line <= 5; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/existing-oversized.rs"
expect_ratchet_failure 'existing-oversized.rs: oversized file grew from 4 to 5 lines'
git -C "${RUST_RATCHET_FIXTURE}" checkout -q -- existing-oversized.rs

awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/boundary.rs"
expect_ratchet_failure 'boundary.rs: grew from 3 to 4 lines and now exceeds limit 3'
git -C "${RUST_RATCHET_FIXTURE}" checkout -q -- boundary.rs

awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/empty.rs"
expect_ratchet_failure 'empty.rs: grew from 0 to 4 lines and now exceeds limit 3'
git -C "${RUST_RATCHET_FIXTURE}" checkout -q -- empty.rs

awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/new.rs"
expect_ratchet_failure 'new.rs: new oversized file has 4 lines (limit 3)'

# Vendored upstream source is explicitly separated from first-party size
# policy; the failures above prove ordinary source cannot use this exemption.
rm -- "${RUST_RATCHET_FIXTURE}/new.rs"
mkdir -p "${RUST_RATCHET_FIXTURE}/third_party/upstream"
awk 'BEGIN { for (line = 1; line <= 4; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/third_party/upstream/generated.rs"
"${RUST_FILE_SIZE_RATCHET}" --root "${RUST_RATCHET_FIXTURE}" \
    --baseline-ref "${RUST_RATCHET_BASELINE}" --max-lines 3 \
    > "${TEST_LOG_DIR}/rust-file-size-ratchet-third-party-pass.log"

# A newline is a valid filename byte. Growing this existing file must still be
# attributed to one baseline entry rather than being split into two records.
awk 'BEGIN { for (line = 1; line <= 5; line += 1) print "// line" }' \
    > "${RUST_RATCHET_FIXTURE}/${ODD_RUST_FILE}"
expect_ratchet_failure 'break.rs: oversized file grew from 4 to 5 lines'

# This also catches a stale lockfile after workspace manifests change.
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
expect_policy_failure bad-cast-ptr-alignment clippy::cast_ptr_alignment
expect_policy_failure bad-cfg-not-test clippy::cfg_not_test
expect_policy_failure bad-dbg clippy::dbg_macro
expect_policy_failure bad-exit clippy::exit
expect_policy_failure bad-ignored-result clippy::let_underscore_must_use
expect_policy_failure bad-fallible-impl-from clippy::fallible_impl_from
expect_policy_failure bad-fn-to-numeric-cast-any clippy::fn_to_numeric_cast_any
expect_policy_failure bad-large-stack-array clippy::large_stack_arrays
expect_policy_failure bad-large-value clippy::large_types_passed_by_value
expect_policy_failure bad-non-send-field clippy::non_send_fields_in_send_ty
expect_policy_failure bad-expect clippy::expect_used
expect_policy_failure bad-panic clippy::panic
expect_policy_failure bad-path-buf-push-overwrite clippy::path_buf_push_overwrite
expect_policy_failure bad-redundant-clone clippy::redundant_clone
expect_policy_failure bad-string-slice clippy::string_slice
expect_policy_failure bad-todo clippy::todo
expect_policy_failure bad-transmute-ptr-to-ptr clippy::transmute_ptr_to_ptr
expect_policy_failure bad-transmute-undefined-repr clippy::transmute_undefined_repr
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
