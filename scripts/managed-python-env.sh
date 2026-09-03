#!/usr/bin/env bash

# Source this file before compiling or running RuViE. PyO3 and the embedded
# runtime must resolve the same pinned CPython distribution.
RUVIE_CPYTHON_VERSION="${RUVIE_CPYTHON_VERSION:-3.13.14}"

if ! command -v uv >/dev/null 2>&1; then
    echo "uv is required to resolve RuViE CPython ${RUVIE_CPYTHON_VERSION}" >&2
    echo "Install uv, then run scripts/bootstrap-managed-python.sh" >&2
    return 1
fi

RUVIE_MANAGED_PYTHON="$({ uv python find --no-project "${RUVIE_CPYTHON_VERSION}"; } 2>/dev/null)" || {
    echo "RuViE CPython ${RUVIE_CPYTHON_VERSION} is not installed" >&2
    echo "Run scripts/bootstrap-managed-python.sh" >&2
    return 1
}
RUVIE_PYTHON_HOME="$(${RUVIE_MANAGED_PYTHON} -c 'import sys; print(sys.base_prefix)')"

export PYO3_PYTHON="${RUVIE_MANAGED_PYTHON}"
export RUVIE_PYTHON_HOME

# uv's standalone CPython keeps its shared library under the managed prefix.
# PyO3 records that dynamic dependency in Rust test binaries, so Linux/macOS
# must be able to resolve it when the binaries start (not only while linking).
case "$(uname -s)" in
    Linux)
        export LD_LIBRARY_PATH="${RUVIE_PYTHON_HOME}/lib${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
        ;;
    Darwin)
        export DYLD_LIBRARY_PATH="${RUVIE_PYTHON_HOME}/lib${DYLD_LIBRARY_PATH:+:${DYLD_LIBRARY_PATH}}"
        ;;
esac

