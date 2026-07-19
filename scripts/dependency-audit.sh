#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
TOOL_ROOT="${QUALITY_TOOL_ROOT:-${REPOSITORY_ROOT}/target/quality-tools}"
TOOL_BIN="${TOOL_ROOT}/bin"
TOOL_BUILD_DIR="${TOOL_ROOT}/build"
EXCEPTION_FILE="${QUALITY_ADVISORY_EXCEPTION_FILE:-${REPOSITORY_ROOT}/quality/dependency-advisory-exceptions.txt}"

readonly CARGO_AUDIT_VERSION="0.22.2"
readonly CARGO_DENY_VERSION="0.20.2"
readonly CARGO_MACHETE_VERSION="0.9.2"

install_tool() {
    local package="$1"
    local binary="$2"
    local expected_version="$3"
    local actual_version=""

    if [[ -x "${TOOL_BIN}/${binary}" ]]; then
        actual_version="$("${TOOL_BIN}/${binary}" --version | awk '{print $NF}')"
    fi

    if [[ "${actual_version}" == "${expected_version}" ]]; then
        return
    fi

    echo "[dependencies] installing ${package} ${expected_version}"
    mkdir -p -- "${TOOL_ROOT}" "${TOOL_BUILD_DIR}"
    CARGO_TARGET_DIR="${TOOL_BUILD_DIR}" cargo install \
        "${package}" \
        --version "${expected_version}" \
        --locked \
        --force \
        --root "${TOOL_ROOT}"

    actual_version="$("${TOOL_BIN}/${binary}" --version | awk '{print $NF}')"
    if [[ "${actual_version}" != "${expected_version}" ]]; then
        echo "${binary}: expected ${expected_version}, got ${actual_version}" >&2
        return 1
    fi
}

load_advisory_exceptions() {
    local today
    local advisory_id
    local review_deadline
    local package_spec
    local direct_parent_spec
    local dependency_path
    local runtime_reachability
    local remediation
    local seen_ids=" "

    today="$(date -u +%F)"
    AUDIT_IGNORE_ARGS=()

    EXCEPTION_PACKAGE_SPECS=()
    EXCEPTION_PARENT_SPECS=()

    while IFS='|' read -r advisory_id review_deadline package_spec direct_parent_spec dependency_path runtime_reachability remediation; do
        [[ -z "${advisory_id}" || "${advisory_id}" == \#* ]] && continue

        if [[ ! "${advisory_id}" =~ ^RUSTSEC-[0-9]{4}-[0-9]{4}$ ]]; then
            echo "invalid advisory ID in ${EXCEPTION_FILE}: ${advisory_id}" >&2
            return 1
        fi
        if ! is_valid_iso_date "${review_deadline}"; then
            echo "invalid review deadline for ${advisory_id}: ${review_deadline}" >&2
            return 1
        fi
        if [[ ! "${package_spec}" =~ ^[A-Za-z0-9_-]+@[0-9][A-Za-z0-9.+-]*$ ]]; then
            echo "invalid exact package for ${advisory_id}: ${package_spec}" >&2
            return 1
        fi
        if [[ ! "${direct_parent_spec}" =~ ^[A-Za-z0-9_-]+@[0-9][A-Za-z0-9.+-]*$ ]]; then
            echo "invalid direct parent for ${advisory_id}: ${direct_parent_spec}" >&2
            return 1
        fi
        if [[ -z "${dependency_path}" || -z "${runtime_reachability}" || -z "${remediation}" ]]; then
            echo "incomplete exception record for ${advisory_id}" >&2
            return 1
        fi
        if [[ "${seen_ids}" == *" ${advisory_id} "* ]]; then
            echo "duplicate advisory exception: ${advisory_id}" >&2
            return 1
        fi
        if [[ "${today}" > "${review_deadline}" ]]; then
            echo "advisory exception ${advisory_id} expired on ${review_deadline}" >&2
            return 1
        fi

        seen_ids="${seen_ids}${advisory_id} "
        AUDIT_IGNORE_ARGS+=(--ignore "${advisory_id}")
        EXCEPTION_PACKAGE_SPECS+=("${package_spec}")
        EXCEPTION_PARENT_SPECS+=("${direct_parent_spec}")
    done <"${EXCEPTION_FILE}"
}

is_valid_iso_date() {
    local value="$1"
    local year
    local month
    local day
    local maximum_day

    [[ "${value}" =~ ^([0-9]{4})-([0-9]{2})-([0-9]{2})$ ]] || return 1
    year=$((10#${BASH_REMATCH[1]}))
    month=$((10#${BASH_REMATCH[2]}))
    day=$((10#${BASH_REMATCH[3]}))

    case "${month}" in
        1|3|5|7|8|10|12) maximum_day=31 ;;
        4|6|9|11) maximum_day=30 ;;
        2)
            maximum_day=28
            if ((year % 400 == 0 || (year % 4 == 0 && year % 100 != 0))); then
                maximum_day=29
            fi
            ;;
        *) return 1 ;;
    esac

    ((day >= 1 && day <= maximum_day))
}

verify_advisory_paths() {
    local index
    local package_spec
    local expected_parent
    local inverse_tree
    local actual_parents

    for ((index = 0; index < ${#EXCEPTION_PACKAGE_SPECS[@]}; index += 1)); do
        package_spec="${EXCEPTION_PACKAGE_SPECS[index]}"
        expected_parent="${EXCEPTION_PARENT_SPECS[index]}"
        inverse_tree="$(cargo tree \
            --locked \
            --target all \
            --workspace \
            --all-features \
            --prefix depth \
            --format '{p}' \
            --invert "${package_spec}")"
        actual_parents="$(printf '%s\n' "${inverse_tree}" | awk '
            /^1[A-Za-z0-9_-]/ {
                line = substr($0, 2)
                split(line, fields, " ")
                version = fields[2]
                sub(/^v/, "", version)
                print fields[1] "@" version
            }
        ')"

        if [[ "${actual_parents}" != "${expected_parent}" ]]; then
            echo "advisory exception path changed for ${package_spec}" >&2
            echo "expected only direct parent: ${expected_parent}" >&2
            echo "actual direct parents: ${actual_parents:-<none>}" >&2
            return 1
        fi
    done
}

cd "${REPOSITORY_ROOT}"
load_advisory_exceptions

if [[ "${QUALITY_AUDIT_VALIDATE_ONLY:-0}" == "1" ]]; then
    echo "[dependencies] advisory exception records are valid"
    exit 0
fi

verify_advisory_paths
install_tool cargo-audit cargo-audit "${CARGO_AUDIT_VERSION}"
install_tool cargo-deny cargo-deny "${CARGO_DENY_VERSION}"
install_tool cargo-machete cargo-machete "${CARGO_MACHETE_VERSION}"

echo "[dependencies] RustSec advisories"
"${TOOL_BIN}/cargo-audit" audit \
    --file Cargo.lock \
    --deny warnings \
    --color never \
    "${AUDIT_IGNORE_ARGS[@]}"

echo "[dependencies] licenses, bans, and sources"
"${TOOL_BIN}/cargo-deny" \
    --log-level warn \
    --locked \
    --all-features \
    --color never \
    check bans licenses sources \
    --show-stats

echo "[dependencies] unused direct dependencies"
"${TOOL_BIN}/cargo-machete" --with-metadata --skip-target-dir .

echo "[dependencies] passed"
