#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

# shellcheck source=scripts/managed-python-env.sh
source "${SCRIPT_DIR}/managed-python-env.sh"

cd "${REPOSITORY_ROOT}"

dependency_tree="$(cargo tree --workspace --edges normal --locked)"
for package in ruvie-python-runtime pyo3 pyo3-ffi; do
    if ! grep -Eq "(^|[[:space:]])${package} v" <<<"${dependency_tree}"; then
        echo "managed CPython dependency graph is missing ${package}" >&2
        exit 1
    fi
done

actual_version="$(${RUVIE_MANAGED_PYTHON} -c 'import platform; print(platform.python_version())')"
if [[ "${actual_version}" != "${RUVIE_CPYTHON_VERSION}" ]]; then
    echo "managed CPython version mismatch: expected ${RUVIE_CPYTHON_VERSION}, got ${actual_version}" >&2
    exit 1
fi

echo "[python] managed CPython ${actual_version}: ${RUVIE_PYTHON_HOME}"

