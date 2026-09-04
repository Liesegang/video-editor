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

SOURCE_ROOT="${CHECK_ROOT}"
if [[ -d "${CHECK_ROOT}/crates" ]]; then
    SOURCE_ROOT="${CHECK_ROOT}/crates"
fi

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

if grep -REn '(^|[^[:alnum:]_])(app|library|uuid)::' \
    "${SOURCE_ROOT}/node-editor-ui/src" "${SOURCE_ROOT}/node-editor-ui/tests"; then
    echo "node-editor-ui sources must use opaque host IDs and contain no RuViE domain imports" >&2
    exit 1
fi

app_tree="$(cargo tree -p app --edges normal --locked --prefix none)"
if ! grep -Eq '^node-editor-ui v' <<<"${app_tree}"; then
    echo "app must consume the shared production node-editor-ui surface" >&2
    exit 1
fi

surface_path="${SOURCE_ROOT}/app/src/ui/panels/node_editor/module_document/surface.rs"
viewer_path="${SOURCE_ROOT}/app/src/ui/panels/node_editor/module_document/viewer.rs"
theme_path="${SOURCE_ROOT}/app/src/ui/panels/node_editor/components/theme.rs"
if [[ -d "${SOURCE_ROOT}/app/src/ui/panels/node_editor" ]]; then
    if [[ ! -f "${surface_path}" ]] \
        || ! grep -Fq 'ModuleSurfaceProjection' "${surface_path}" \
        || ! grep -Fq 'Editor::interact' "${surface_path}" \
        || ! grep -Fq 'InteractionOptions::SELECTION_AND_MOVE' "${surface_path}"; then
        echo "app must drive Node selection through the shared production surface" >&2
        exit 1
    fi
    for production_chrome_call in \
        'Editor::node_frame' \
        'Editor::node_header_frame' \
        'Editor::show_node_header' \
        'Editor::show_port_label'; do
        if ! grep -Fq "${production_chrome_call}" "${viewer_path}"; then
            echo "app must render production Node chrome through ${production_chrome_call}" >&2
            exit 1
        fi
    done
    for production_theme_call in 'Editor::port_visual_style'; do
        if ! grep -Fq "${production_theme_call}" "${theme_path}"; then
            echo "app must render production port chrome through ${production_theme_call}" >&2
            exit 1
        fi
    done
fi

if [[ -d "${SOURCE_ROOT}/app/src/ui/module_node_editor" ]] \
    || grep -REn 'module_node_editor|ModuleNodeEditor' \
        "${SOURCE_ROOT}/app/src" --include='*.rs'; then
    echo "app must keep one Node Editor surface for bounded Module documents" >&2
    exit 1
fi

echo "[quality] node-editor-ui dependency boundary passed"
