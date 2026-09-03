#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
CHECK_ROOT="${REPOSITORY_ROOT}"

if (( $# > 0 )); then
    if [[ $# -ne 2 || "$1" != "--root" || -z "$2" ]]; then
        echo "usage: $0 [--root PATH]" >&2
        exit 2
    fi
    CHECK_ROOT="$(cd -- "$2" && pwd)"
fi

cd "${CHECK_ROOT}"

node_editor_tree="$(
    cargo tree \
        -p node-editor-ui \
        --all-features \
        --target all \
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

if grep -REn '(^|[^[:alnum:]_])(app|library|uuid)::' node-editor-ui/src node-editor-ui/tests; then
    echo "node-editor-ui sources must use opaque host IDs and contain no RuViE domain imports" >&2
    exit 1
fi

app_tree="$(cargo tree -p app --no-default-features --edges normal --locked --prefix none)"
if grep -Eq '^node-editor-ui v' <<<"${app_tree}"; then
    echo "the no-default-features basic editor must not depend on the optional Node editor package" >&2
    exit 1
fi

if grep -REn 'structural_merge_node_id|NodeContent::CompositionInstance|model::project::Project' \
    app/src library/src; then
    echo "production code must not recover the retired graph-owned Timeline model" >&2
    exit 1
fi

if ! grep -Fq 'Timeline items are never expanded into nodes' app/src/timeline_app.rs; then
    echo "the Logic workspace must state and retain the Timeline/Module boundary" >&2
    exit 1
fi

echo "[quality] Timeline/Module UI boundary passed"
