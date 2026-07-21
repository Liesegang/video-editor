#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"

cd "${REPOSITORY_ROOT}"

node_editor_tree="$(
    cargo tree \
        -p node-editor-ui \
        --edges normal,build,dev \
        --locked \
        --prefix none
)"
for forbidden_package in app library; do
    if grep -Eq "^${forbidden_package} v" <<<"${node_editor_tree}"; then
        echo "node-editor-ui must not depend on ${forbidden_package}" >&2
        exit 1
    fi
done

app_tree="$(cargo tree -p app --edges normal --locked --prefix none)"
if ! grep -Eq '^node-editor-ui v' <<<"${app_tree}"; then
    echo "app must consume node-editor-ui through its adapter boundary" >&2
    exit 1
fi

echo "[quality] node-editor-ui dependency boundary passed"
