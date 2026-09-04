# Repository implementation rules

## Reuse before adding infrastructure

- Search the repository and relevant Git history before adding any component, module, service, model, runtime, renderer, persistence path, build task, editor surface, or other infrastructure.
- Extend the existing production implementation when it already owns the responsibility. Do not create a parallel implementation, a lightly renamed copy, or an adapter that duplicates the existing behavior.
- If a change requires an existing implementation to support a new model or use case, generalize it at its real abstraction boundary and delete the superseded implementation in the same change.
- A second implementation is allowed only when the interaction semantics are genuinely different and the reason is documented in an ADR.

## Shared editor surfaces

- Node modules use the production Node Editor surface. Do not add a second editor surface for modules.
- Timeline, Curve Editor, Node Editor, and Preview navigation must use `pan-zoom-ui` and the shared application viewport controller. Content, grid, hit testing, overlays, and QA metadata must derive from the same `CanvasState` transform.
- Dialogs must use the shared modal/dialog primitives and shared footer/action layout. Extend those primitives when a required behavior is missing instead of hand-building another window.

## Repository hygiene

- Apply DRY across UI, models, services, runtime, rendering, persistence, tests, and build tooling. Shared policy and transformation logic belongs in a named module or crate with one authoritative implementation.
- Keep one authoritative implementation for each responsibility and remove dead compatibility paths, duplicated helpers, copied constants, and obsolete modules while changing that responsibility.
- Do not hide duplication behind forwarding wrappers or similarly shaped adapters. Extract the actual shared abstraction and keep domain-specific code limited to data access and intent handling.
- Name modules after their lasting domain responsibility, not migration history. Avoid names such as `new`, `legacy`, `timeline_first`, or `v2` unless they identify an actual persisted format/version boundary.
- Use `Node Editor` for processing-node topology and `Curve Editor` for keyframe/interpolation curves. Do not label curve editing with graph terminology.
- Split source, test, and QA files before they exceed 1,000 lines. Split by coherent responsibility rather than creating numbered or arbitrary fragments.
- Do not commit PowerShell (`.ps1`) scripts. Repository automation must use Rust tasks or portable shell scripts; PowerShell is acceptable only for local, uncommitted debugging.
- For native UI changes, exercise the affected interaction through the loopback HTTP QA bridge and verify rendered output, state changes, and error logs before declaring completion.
