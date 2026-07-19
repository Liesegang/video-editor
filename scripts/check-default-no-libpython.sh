#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

cd "${REPOSITORY_ROOT}"

dependency_tree="$(cargo tree --workspace --edges normal --locked)"
if grep -Eq '(^|[[:space:]])(pyo3|pyo3-ffi|pyo3-build-config) v' <<<"${dependency_tree}"; then
    echo "default workspace dependency graph unexpectedly contains PyO3/libpython" >&2
    exit 1
fi

echo "[python] default workspace graph is independent of PyO3/libpython"
