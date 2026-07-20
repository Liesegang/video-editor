#!/usr/bin/env bash
set -euo pipefail

MAX_LINES=1000
SCAN_ROOT=""

usage() {
    echo "usage: $0 [--root DIRECTORY] [--max-lines COUNT]" >&2
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --root)
            [[ $# -ge 2 ]] || { usage; exit 2; }
            SCAN_ROOT="$2"
            shift 2
            ;;
        --max-lines)
            [[ $# -ge 2 ]] || { usage; exit 2; }
            MAX_LINES="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage
            exit 2
            ;;
    esac
done

case "${MAX_LINES}" in
    ''|*[!0-9]*)
        echo "Rust file line limit must be a positive integer: ${MAX_LINES}" >&2
        exit 2
        ;;
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

rust_files() {
    if git -C "${SCAN_ROOT}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        git -C "${SCAN_ROOT}" ls-files \
            --cached \
            --others \
            --exclude-standard \
            -z \
            -- '*.rs'
    else
        find "${SCAN_ROOT}" -type f -name '*.rs' -print0
    fi
}

checked=0
violations=0
while IFS= read -r -d '' rust_file; do
    if [[ "${rust_file}" = /* ]]; then
        absolute_path="${rust_file}"
        display_path="${rust_file#"${SCAN_ROOT}"/}"
    else
        absolute_path="${SCAN_ROOT}/${rust_file}"
        display_path="${rust_file}"
    fi

    [[ -f "${absolute_path}" ]] || continue
    line_count="$(awk 'END { print NR + 0 }' "${absolute_path}")"
    checked=$((checked + 1))
    if [[ "${line_count}" -gt "${MAX_LINES}" ]]; then
        printf '%s: %d lines (limit %d)\n' \
            "${display_path}" "${line_count}" "${MAX_LINES}" >&2
        violations=$((violations + 1))
    fi
done < <(rust_files)

if [[ "${violations}" -ne 0 ]]; then
    printf 'Rust file size gate failed: %d of %d files exceed %d lines\n' \
        "${violations}" "${checked}" "${MAX_LINES}" >&2
    exit 1
fi

printf '[quality] Rust file size gate passed: %d files, limit %d lines\n' \
    "${checked}" "${MAX_LINES}"
