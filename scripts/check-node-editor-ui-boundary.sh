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

app_tree="$(cargo tree -p app --edges normal --locked --prefix none)"
if ! grep -Eq '^node-editor-ui v' <<<"${app_tree}"; then
    echo "app must consume node-editor-ui through its adapter boundary" >&2
    exit 1
fi

adapter_path="app/src/ui/panels/node_editor/surface.rs"
panel_path="app/src/ui/panels/node_editor/panel.rs"
viewer_path="app/src/ui/panels/node_editor/viewer/snarl.rs"
theme_path="app/src/ui/panels/node_editor/components/theme.rs"
if [[ -d "app/src/ui/panels/node_editor" ]]; then
    if [[ ! -f "${adapter_path}" ]] \
        || ! grep -Fq 'SurfaceProjection' "${adapter_path}" \
        || ! grep -Fq 'node_editor_ui::Editor::interact' "${panel_path}" \
        || ! grep -Fq 'InteractionOptions::SELECTION' "${panel_path}"; then
        echo "app must drive production Node selection through the node-editor-ui frame adapter" >&2
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
    for production_theme_call in \
        'Editor::paint_group_backdrop' \
        'Editor::paint_group_foreground' \
        'Editor::port_visual_style'; do
        if ! grep -Fq "${production_theme_call}" "${theme_path}"; then
            echo "app must render production group/port chrome through ${production_theme_call}" >&2
            exit 1
        fi
    done
fi

echo "[quality] node-editor-ui dependency boundary passed"
