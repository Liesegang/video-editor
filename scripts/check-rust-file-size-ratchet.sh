#!/usr/bin/env bash
set -euo pipefail

MAX_LINES=1000
BASELINE_REF="${RUST_FILE_SIZE_BASE_REF:-}"
SCAN_ROOT=""

usage() {
    echo "usage: $0 [--root DIRECTORY] [--baseline-ref GIT_REF] [--max-lines COUNT]" >&2
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --root) [[ $# -ge 2 ]] || { usage; exit 2; }; SCAN_ROOT="$2"; shift 2 ;;
        --baseline-ref) [[ $# -ge 2 ]] || { usage; exit 2; }; BASELINE_REF="$2"; shift 2 ;;
        --max-lines) [[ $# -ge 2 ]] || { usage; exit 2; }; MAX_LINES="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) usage; exit 2 ;;
    esac
done

case "${MAX_LINES}" in
    ''|*[!0-9]*) echo "Rust file line limit must be a positive integer: ${MAX_LINES}" >&2; exit 2 ;;
esac
if [[ "${MAX_LINES}" -eq 0 ]]; then
    echo "Rust file line limit must be greater than zero" >&2
    exit 2
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if [[ -z "${SCAN_ROOT}" ]]; then
    SCAN_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
elif [[ ! -d "${SCAN_ROOT}" ]]; then
    echo "Rust file scan root is not a directory: ${SCAN_ROOT}" >&2
    exit 2
else
    SCAN_ROOT="$(cd -- "${SCAN_ROOT}" && pwd)"
fi

if ! git -C "${SCAN_ROOT}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "Rust file size ratchet requires a Git worktree: ${SCAN_ROOT}" >&2
    exit 2
fi

if [[ -z "${BASELINE_REF}" ]]; then
    if git -C "${SCAN_ROOT}" rev-parse --verify --quiet 'origin/main^{commit}' >/dev/null; then
        BASELINE_REF=origin/main
    elif git -C "${SCAN_ROOT}" rev-parse --verify --quiet 'main^{commit}' >/dev/null; then
        BASELINE_REF=main
    else
        echo "Rust file size baseline is required; pass --baseline-ref or set RUST_FILE_SIZE_BASE_REF" >&2
        exit 2
    fi
fi
if ! git -C "${SCAN_ROOT}" rev-parse --verify --quiet "${BASELINE_REF}^{commit}" >/dev/null; then
    echo "Rust file size baseline is not a commit: ${BASELINE_REF}" >&2
    exit 2
fi
if ! git -C "${SCAN_ROOT}" merge-base --is-ancestor "${BASELINE_REF}" HEAD; then
    echo "Rust file size baseline must be an ancestor of HEAD: ${BASELINE_REF}" >&2
    exit 2
fi

checked=0
violations=0
while IFS= read -r -d '' rust_file; do
    # Audited upstream snapshots live under third_party and retain their source
    # layout. First-party Rust remains subject to the 1,000-line ratchet.
    case "${rust_file}" in
        third_party/*) continue ;;
    esac
    absolute_path="${SCAN_ROOT}/${rust_file}"
    [[ -f "${absolute_path}" ]] || continue
    current_lines="$(awk 'END { print NR + 0 }' "${absolute_path}")"
    baseline_count=0
    baseline_has_file=false
    # Query the exact Git path instead of keeping a Bash associative array.
    # macOS still ships Bash 3.2, and the NUL-delimited outer loop keeps spaces,
    # newlines, leading dashes, and other valid filename bytes intact.
    if git -C "${SCAN_ROOT}" cat-file -e \
        "${BASELINE_REF}:${rust_file}" 2>/dev/null; then
        baseline_has_file=true
        baseline_count="$(
            git -C "${SCAN_ROOT}" show "${BASELINE_REF}:${rust_file}" |
                awk 'END { print NR + 0 }'
        )"
    fi
    checked=$((checked + 1))

    if [[ "${current_lines}" -le "${MAX_LINES}" ]]; then
        continue
    fi
    if [[ "${baseline_has_file}" == false ]]; then
        printf '%s: new oversized file has %d lines (limit %d)\n' \
            "${rust_file}" "${current_lines}" "${MAX_LINES}" >&2
        violations=$((violations + 1))
    elif [[ "${baseline_count}" -le "${MAX_LINES}" ]]; then
        printf '%s: grew from %d to %d lines and now exceeds limit %d\n' \
            "${rust_file}" "${baseline_count}" "${current_lines}" "${MAX_LINES}" >&2
        violations=$((violations + 1))
    elif [[ "${current_lines}" -gt "${baseline_count}" ]]; then
        printf '%s: oversized file grew from %d to %d lines (limit %d)\n' \
            "${rust_file}" "${baseline_count}" "${current_lines}" "${MAX_LINES}" >&2
        violations=$((violations + 1))
    fi
done < <(
    git -C "${SCAN_ROOT}" ls-files --cached --others --exclude-standard -z -- '*.rs'
)

if [[ "${violations}" -ne 0 ]]; then
    printf 'Rust file size ratchet failed against %s: %d of %d files regressed\n' \
        "${BASELINE_REF}" "${violations}" "${checked}" >&2
    exit 1
fi

printf '[quality] Rust file size ratchet passed: %d files against %s, limit %d lines\n' \
    "${checked}" "${BASELINE_REF}" "${MAX_LINES}"
