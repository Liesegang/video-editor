#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=scripts/managed-python-env.sh
source "${SCRIPT_DIR}/managed-python-env.sh"

if [[ "$#" -eq 0 ]]; then
    echo "usage: $0 COMMAND [ARGUMENT ...]" >&2
    exit 2
fi

exec "$@"

