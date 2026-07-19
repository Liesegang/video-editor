#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

if ! command -v jq >/dev/null 2>&1; then
    echo "lint inventory requires jq" >&2
    exit 2
fi

cd "${REPOSITORY_ROOT}"

echo '| lint | findings |'
echo '| --- | ---: |'
cargo clippy \
    --workspace \
    --all-targets \
    --all-features \
    --locked \
    --message-format=json \
    -- \
    --cap-lints warn \
    -W clippy::pedantic \
    -W clippy::nursery \
    -W clippy::restriction \
    -A clippy::blanket_clippy_restriction_lints \
    2>/dev/null \
    | jq -r '
        select(.reason == "compiler-message")
        | .message
        | select(.level == "warning" and .code.code != null)
        | .code.code
      ' \
    | sort \
    | uniq -c \
    | sort -k2 \
    | awk '{ printf "| `%s` | %d |\n", $2, $1 }'
