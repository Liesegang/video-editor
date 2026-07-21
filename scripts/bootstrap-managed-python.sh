#!/usr/bin/env bash
set -euo pipefail

RUVIE_CPYTHON_VERSION="${RUVIE_CPYTHON_VERSION:-3.13.14}"

if ! command -v uv >/dev/null 2>&1; then
    echo "uv is required to install RuViE CPython ${RUVIE_CPYTHON_VERSION}" >&2
    exit 1
fi

uv python install "${RUVIE_CPYTHON_VERSION}"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/managed-python-env.sh
source "${SCRIPT_DIR}/managed-python-env.sh"
"${RUVIE_MANAGED_PYTHON}" --version
