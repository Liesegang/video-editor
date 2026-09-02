#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
PROBE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ruvie-node-api.XXXXXX")"
trap 'rm -rf -- "${PROBE_ROOT}"' EXIT

mkdir -p "${PROBE_ROOT}/src/bin"

cat >"${PROBE_ROOT}/Cargo.toml" <<EOF
[package]
name = "ruvie-node-authoring-api-probe"
version = "0.0.0"
edition = "2024"

[dependencies]
library = { path = "${REPOSITORY_ROOT}/library" }
EOF

cat >"${PROBE_ROOT}/src/bin/pass.rs" <<'EOF'
use library::model::frame::color::Color;
use library::model::authoring::TimelineInterval;
use library::plugin::PluginManager;
use library::TimelineEditorService;

fn main() -> Result<(), String> {
    let service = TimelineEditorService::create_default("external authoring probe")
        .map_err(|error| error.to_string())?;
    let project = service.snapshot().map_err(|error| error.to_string())?;
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);
    let (solid, _) = service
        .add_solid(
            track_id,
            Color::white(),
            TimelineInterval::new(0.0, 2.0)?,
            0,
        )
        .map_err(|error| error.to_string())?;
    service
        .attach_effect(solid, "blur", &PluginManager::default())
        .map_err(|error| error.to_string())?;
    Ok(())
}
EOF

cat >"${PROBE_ROOT}/src/bin/fail_property_map_mut.rs" <<'EOF'
use library::editor::handlers::property_ops::property_map_mut;

fn main() {
    let _ = property_map_mut;
}
EOF

cat >"${PROBE_ROOT}/src/bin/fail_node_fields.rs" <<'EOF'
use library::model::Node;
use library::model::property::PropertyMap;

fn main() {
    let mut node = Node::new_merge("raw mutation must stay sealed");
    node.properties = PropertyMap::new();
}
EOF

export CARGO_TARGET_DIR="${REPOSITORY_ROOT}/target"

echo "[node-api] supported Timeline-first authoring compiles"
cargo check --quiet --offline --manifest-path "${PROBE_ROOT}/Cargo.toml" --bin pass

expect_compile_failure() {
    local binary_name="$1"
    local expected_pattern="$2"
    local diagnostics="${PROBE_ROOT}/${binary_name}.log"
    if cargo check --offline --manifest-path "${PROBE_ROOT}/Cargo.toml" \
        --bin "${binary_name}" >"${diagnostics}" 2>&1; then
        echo "[node-api] ${binary_name} unexpectedly compiled" >&2
        return 1
    fi
    if ! grep -Eq "${expected_pattern}" "${diagnostics}"; then
        echo "[node-api] ${binary_name} failed for an unexpected reason" >&2
        cat "${diagnostics}" >&2
        return 1
    fi
}

echo "[node-api] raw handler map access is sealed"
expect_compile_failure fail_property_map_mut 'module `property_ops` is private|no `property_map_mut`'

echo "[node-api] Node property storage is sealed"
expect_compile_failure fail_node_fields 'field `properties` of struct .*Node` is private'

echo "[node-api] passed"
