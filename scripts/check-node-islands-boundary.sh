#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
AUTHORING_ROOT="${REPOSITORY_ROOT}/library/src/model/authoring"
PLAN_ROOT="${REPOSITORY_ROOT}/library/src/core/render_plan"

if [[ ! -d "${AUTHORING_ROOT}" || ! -d "${PLAN_ROOT}" ]]; then
    echo "Node-islands authoring and RenderPlan roots must exist" >&2
    exit 1
fi

forbidden='structural_merge_node_id|structural_sound_merge_node_id|NodeContainer|CompositionInstanceContent'
if rg -n --glob '*.rs' --glob '!tests.rs' --glob '!**/tests/**' \
    "${forbidden}" "${AUTHORING_ROOT}" "${PLAN_ROOT}"; then
    echo "Timeline-first authoring or RenderPlan depends on retired graph-owned structure" >&2
    exit 1
fi

if ! rg -q 'Module\(ModuleInvocation\)' "${AUTHORING_ROOT}"; then
    echo "Timeline SourceRef must expose an explicit Module invocation for Node Clips" >&2
    exit 1
fi

if rg -n 'pub .*node_ids|pub .*output_node_id' "${AUTHORING_ROOT}"; then
    echo "Timeline authoring types must not own Module topology or Node outputs" >&2
    exit 1
fi

echo "[boundary] Timeline items and Module topology remain separate"
