# Cloud runner handoff — 2026-07-20

This handoff freezes the local multi-agent session at pushed, reproducible
checkpoints. Do not merge the feature branches merely because they are pushed;
each branch still has explicit integration blockers below.

## Repository checkpoint

- Repository: `Liesegang/video-editor`
- Working branch: `main`
- Implementation baseline before this handoff document:
  `aa3a402ecd150adb0ed9f0ce2a682f2d152e2ab5`. The pushed handoff-document
  commit is its direct descendant; do not reset it away.
- Main worktree: clean
- Important main commits:
  - `38a1579 fix(node-editor): clear wire selection`
  - `117d384 refactor(node-editor): isolate canvas policy`
  - `aa3a402 test(quality): add Rust file size policy checker`
- The legacy Project-graph wire suite described by this historical handoff was
  retired with that editing model. Current native UI coverage is listed in
  `crates/app/src/qa/README.md` and runs through
  `python3 scripts/qa-runner.py --mode full`.

All branches in the next table are pushed, have an upstream, and were clean at
handoff time.

| Work | Remote branch | HEAD |
| --- | --- | --- |
| Node Editor physical split | `origin/codex/node-editor-modular-38a1579` | `f961428d32f840cc7245b30b420e70977dfedd93` |
| Transform / Preview routing | `origin/codex/transform-style-fe333f6` | `5a060d602357c186a80598ade9c6f744eb1b1816` |
| Python Expression foundation | `origin/codex/python-expression-foundation` | `4dbd0cb428ef7f3f8f7a2b02ea050756a69871b8` |
| Fmod / arithmetic / vector metadata | `origin/codex/fmod-number-c609c81` | `6f148feb1d682d5f9d724d01d968bb680a68edb3` |

## Non-negotiable architecture

1. One authoritative `Project` is edited by Node Editor, Timeline, Inspector,
   Preview, and Renderer. Do not persist a facade/intermediate graph model.
2. The model is `Project -> Composition -> Track -> Clip`; Composition and
   Track may also directly own Nodes. Clip is the normal Timeline/Inspector
   editing unit.
3. Pre-v1 means no `schema_version`, migration, compatibility alias, or legacy
   serde path. Change the schema directly and update fixtures/tests.
4. A built RuViE host must load plugins built later. Runtime plugin operations
   cannot assume their implementation source was available when the host was
   compiled. Unknown operations must load losslessly and evaluate safely as
   no output.
5. Do not reject a whole user Project merely because a property is incomplete.
   Preserve it, diagnose it, repair deterministically when appropriate, and
   fail the affected evaluation closed.
6. Every Rust source file must ultimately be at most 1000 physical lines. No
   file allowlist. Until all existing violations reach zero, enforce a ratchet:
   no new oversized file and no increase to an existing oversized file.
7. Use small semantic commits and push fixes promptly. An intermediate broken
   commit is acceptable only when a following commit repairs it.
8. Tests may use internal commands to reach setup state, but final UI proof
   must resolve a fresh on-screen rectangle and inject real coordinate click,
   drag, release, or keyboard events through the HTTP QA bridge.

## Priority 0 — make the size policy an actual gate

At the time of this handoff, the original Rust-only size checker and its
boundary self-tests already existed.
The hard scan currently reports the following 19 main-branch violations, so it
is intentionally not yet called by `scripts/quality-gate.sh`:

| Lines | File |
| ---: | --- |
| 16072 | `crates/app/src/ui/panels/node_editor.rs` |
| 4482 | `crates/library/src/plugin/runtime_native.rs` |
| 2865 | `crates/library/tests/project_graph_tests.rs` |
| 2619 | `crates/library/src/model/project/connection.rs` |
| 2505 | `crates/app/src/ui/panels/inspector/mod.rs` |
| 2102 | `crates/library/src/editor/project_service.rs` |
| 2032 | `crates/app/src/ui/panels/preview/mod.rs` |
| 1941 | `crates/app/src/ui/panels/timeline/clip_area/clips.rs` |
| 1758 | `crates/library/src/plugin/manager.rs` |
| 1626 | `crates/library/src/core/framing/frame.rs` |
| 1451 | `crates/library/src/model/project/property.rs` |
| 1439 | `crates/library/src/model/project/mod.rs` |
| 1356 | `crates/library/tests/effector_graph_tests.rs` |
| 1320 | `plugins/random_property/src/lib.rs` |
| 1320 | `crates/library/src/plugin/loaders/ffmpeg_video.rs` |
| 1305 | `crates/library/src/core/rendering/skia_renderer.rs` |
| 1274 | `crates/library/tests/media_fixture_e2e_tests.rs` |
| 1088 | `crates/library/src/core/audio/mixer.rs` |
| 1072 | `crates/app/src/ui/panels/inspector/properties.rs` |

Implement a baseline-ref ratchet checker and self-tests without a path
allowlist. A feature branch must fail if it creates a new `>1000` file or makes
an existing `>1000` file longer. Keep the hard checker as the final rule; once
the inventory reaches zero, call it unconditionally from `quality-gate.sh` and
make its presence fail-closed in `quality-gate-self-test.sh`.

Clippy has `too_many_lines` for functions, not a file/module line-count lint.
Keep strict Clippy as a complementary rule rather than pretending it enforces
this policy.

Completion note (2026-09-05): the inventory reached zero, so the Rust-only
ratchet was replaced by the strict first-party source gate at
`scripts/check-source-file-size.sh`. The CI quality gate now runs it
unconditionally for Rust, Python, portable shell, JavaScript/TypeScript,
C/C++, and SkSL source, tests, and QA automation.

## Priority 1 — independent, loosely coupled Node Editor library

The approved direction is one publishable workspace package named
`node-editor-ui` (crate `node_editor_ui`). Start inside this repository; create
a dependency boundary that permits extraction to another repository later.
Do not create a second persisted Project or graph model.

Data flow:

```text
authoritative Project
        | per-frame descriptors
        v
video-editor adapter ------> node-editor-ui
        ^                          |
        +----- domain-neutral intents
```

The generic crate may own only transient UI state such as viewport, hover,
active gesture, and selected wire. Node positions, connections, group sizes,
entity selection, containment, and history remain authoritative host input on
every frame.

Recommended public boundary:

- opaque domain-neutral `ItemId`, `PortId`, `WireId`, and `TypeKey` newtypes;
- borrowed `GraphFrame` made from Node/Port/Wire/nested-Group descriptors and
  an authoritative selection descriptor;
- transient-only `InteractionState`;
- `NodeEditorUi::show(GraphFrame, ...) -> EditorOutput`;
- domain-neutral intents such as Connect, Disconnect, Move, Delete, Reparent,
  Select, ResizeGroup, and DeselectWire;
- stable QA geometry/value objects, while HTTP/JSON registry publication stays
  in the app;
- a narrow host-renderer extension point for domain-specific Node body widgets.

Keep `Composition`, `Track`, `Clip`, `Time`, `FPS`, `Image`, `Shape`, blend
modes, plugin semantics, `Project`, `EditorService`, `HistoryManager`, and app
`SelectionTarget` out of the crate. Do not keep an authoritative
`egui_snarl::Snarl` cache between frames; an ephemeral projection is acceptable.
The initial crate may depend on egui and egui-snarl. A core+egui two-crate split
is deliberately deferred until a real non-egui consumer exists.

Add an ADR/README that fixes ownership, dependency direction, descriptor to
intent flow, and forbidden dependencies. Enforce that normal, dev, and build
dependencies never point at `app` or the video `library`. Add a fake in-memory
host proving render/connect/move/delete/reparent/wire-deselect behavior without
video types.

### Existing split branch

Start from `origin/codex/node-editor-modular-38a1579`.

- It is rebased on `aa3a402`.
- `cargo check -p app --all-features --locked` passed.
- Focused Node Editor event tests passed 68/68.
- Commits split UI types/viewer/body/property controls, wire
  model/render/hit/gesture, QA geometry, graph queries, panel orchestration,
  layout, commands, resize/selection, merge layers, and wire menus.
- Wire Escape/blank-click/menu-close deselection remains covered.
- `crates/app/src/ui/panels/node_editor/mod.rs` is still about 8339 lines.
- The `node-editor-ui` crate/ADR/fake host are designed but not implemented.

Finish the remaining production and test separation, then move only generic
modules into the crate. The app adapter must absorb Transform, Expression, and
numeric branch changes when this branch is rebased; never restore them to a god
file merely to resolve conflicts.

## Priority 2 — repair and independently review feature branches

Do not merge these branches in parallel. Give each an independent adversarial
review, satisfy the size ratchet, rebase onto the latest main, rerun its full
gates and coordinate E2E, then merge/push main one branch at a time. The Node
Editor library branch should be rebased after the safe domain branches land so
their UI code can be placed in the adapter boundary.

### Transform / Preview — currently blocked

Branch: `origin/codex/transform-style-fe333f6` at `5a060d6`.

Implemented work includes native Shape and Image Transform operations,
content/spatial identity separation, Clip facade versus exact Node routing,
stale-path/ambiguous-owner/singular-transform fail-closed behavior, gizmo QA
geometry, Preview module splitting, and follow-up input-sequencer repaint.

Current blockers:

- Fresh-spawn HTTP E2E still flakes at `Node Editor direct Node selection`.
  It passed twice at `178eae1` but failed again at current HEAD, so the repaint
  fix is not proven sufficient. Existing canonical evidence is not current-HEAD
  evidence. Diagnose component freshness/dock/context lifecycle without fixed
  coordinates or internal mutation shortcuts. Require consecutive clean runs.
- Remaining oversized-file deltas versus `aa3a402`:
  - `node_editor.rs +155`
  - `inspector/mod.rs +73`
  - `frame.rs +115`
  - `project_service.rs +90`
  - `plugin/manager.rs +88`
  Extract branch additions into responsibility-based sibling modules until
  every delta is `<= 0`.
- The current-head full quality gate was not rerun. The full gate passed at
  `178eae1`; the current test-split HEAD only reran 31 related tests.

The branch already reduced the three touched test files relative to main:
effector graph `-38`, creative render `-43`, and media fixture `-30` lines.

### Python Expression — foundation only, not complete UI

Branch: `origin/codex/python-expression-foundation` at `4dbd0cb`.

Implemented work includes a Rust-owned restricted Python evaluator using the
RustPython parser (not arbitrary CPython execution), typed authored fallback
plus diagnostics, exact integer comparison above 2^53, finite vector/numeric
validation, context-aware runtime/audio evaluation, contextless `Result`
sampling, Inspector diagnostics, public API narrowing, and property/audio
module splitting. Default builds must remain free of a libpython dependency.

Current blockers:

- `node_editor.rs +41` and `inspector/properties.rs +2` versus main. Move the
  remaining call-site responsibilities out until both deltas are `<= 0`.
- Strict Clippy, default no-libpython proof, full quality gate, post-rebase full
  tests, and HTTP coordinate GUI QA were not run at final HEAD.
- Inspector Const/Keyframe/Script authoring controls and the explicit Script
  Node UI are not implemented yet.
- Verify the contract: registered Expression syntax/runtime/type errors return
  typed fallback plus a visible scoped diagnostic; missing evaluator/source,
  malformed mode, invalid context, non-finite fallback, or type mismatch fail
  closed as no output/silence. No legacy `Number(0)` escape path.

The branch already reduced `project_service`, `runtime_native`, Inspector mod,
media fixture tests, property, and mixer files; `frame.rs` is unchanged versus
main rather than enlarged.

### Fmod / arithmetic / vector metadata — based on older main

Branch: `origin/codex/fmod-number-c609c81` at `6f148fe`.

It contains generic Fmod, float/vector arithmetic, Node/UI integration, and
Vec2/3/4 min/max/step/suffix/hard-limit metadata through the model, plugin ABI,
runtime descriptors, converters, and random-property plugin. Workspace check,
strict Clippy, targeted tests, plugin check, fmt, and diff-check passed at the
checkpoint.

It is based on `f9d1059`; rebase only after the Node Editor adapter boundary is
clear. Remaining work:

- one shared vertical Vec2/Vec3/Vec4 control with thin Node/Inspector adapters;
- apply min/max/step/suffix/hard-limit metadata in both UIs;
- publish the complete vector metadata, not only suffix;
- enforce finite values and component hard limits in validation;
- preserve descriptor/authored order for unknown/common properties rather than
  alphabetizing fallback ports;
- metadata/default/invalid-range/round-trip tests and real-coordinate drag/Undo
  E2E;
- split the branch's `crates/library/src/model/node/mod.rs` (about 1113 lines) and any
  other touched oversized file.

## Priority 3 — finish the single-model editing experience

The normal Timeline-only workflow must remain complete for novice users. Node
editing is an advanced extension, not a prerequisite.

1. Implement explicit stateless `ClipFacadeQuery` and atomic
   `ClipFacadeCommand` functions over the authoritative Project graph.
2. Timeline/Preview selection exposes Clip semantic properties in Inspector.
   Selecting a Node in Node Editor exposes that exact Node 1:1.
3. Preview visual clicks select the owning Clip, not the hidden content Node.
4. Inspector must add/remove/reorder Style, Decorator, and Effect by rewiring
   the existing graph. Preserve advanced connections; do not replace a chain
   wholesale.
5. Timeline edits must immediately update the same Nodes; Node edits must
   immediately invalidate and repaint Timeline/Preview/Inspector.
6. Profile Preview latency before optimizing. Record the measured hot path and
   prove the improvement.

The intended semantic graph is approximately:

```text
Text/Shape -> optional Decorator/Effector -> Style -> Effect
           -> ordered per-input-blend Merge -> Clip -> Track -> Composition
```

Transform is a separate Shape or Image operation. Text does not own opacity;
appearance belongs to Style. Time is the explicit animated input. FPS and
resolution are read-only context values available to calculations, not
conflicting author inputs. Frame is derived from time and FPS.

## Priority 4 — remaining graph/model/UI behavior

- Redesign Backplate with two Shape inputs: target geometry and arbitrary
  background Shape template. Output background geometry only. Remove color,
  stroke, shape dropdown, and radius from Backplate; Fill/Stroke and Shape
  Generator own appearance/geometry. Support Char/Line/Block grouping, padding,
  and explicit fit/placement.
- Keep ordered variadic Merge inputs visibly ordered and store an independent
  blend mode per incoming wire.
- Implement Node bypass for compatible Image/Shape/scalar/vector types. Keep
  disable/out-of-range evaluation as an explicit harmless optional/no-output
  result rather than a fake universal default value.
- Finish generic scalar/vector arithmetic, explicit Time remapping through
  ordinary Fmod/math Nodes, and implicit read-only FPS context.
- Preserve dynamic plugin ABI behavior for Transform, Opacity Effector, Style,
  Decorator, Effect, and Loader operations.
- Reproduce before changing already-touched bugs: keyframe/Curve Editor edits,
  Timeline Clip move/trim/reorder, Track reorder, Node/wire delete, vector input
  editing, menu scroll/height, header drag, container resize/reparent, and
  real-time Preview invalidation.
- Node Editor UX after the library boundary is stable: LTR initial/auto layout,
  vertical ports, approximately 100x additional zoom-out, Blender-style wire
  connect/cut/splice, hierarchical menu with flat search, stable scrolling,
  generic nested containers, edge-hover resizing, readable headers, and
  non-editable text selection suppression.

## Priority 5 — verification and final quality

Run cheap focused tests after each small commit and full gates at integration
checkpoints. At minimum:

```bash
source scripts/clippy-policy.sh
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets --locked
cargo clippy --workspace --all-features --all-targets --locked -- "${CLIPPY_POLICY_ARGS[@]}"
cargo test -p app --all-features --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
./scripts/check-default-no-libpython.sh
./scripts/quality-gate.sh
```

Use the loopback bridge for final GUI proof. Always obtain the component from a
fresh completed frame immediately before injection. Verify post-state in a
separate later frame. Keep evidence under `target/`; do not treat a stale
evidence hash as proof for a rebased HEAD.

Required E2E coverage includes Node to Timeline/Preview reflection, Timeline to
Node reflection, Clip/Track reorder, Curve Editor keyframes, Inspector edits,
Text/Shape/ensemble rendering, media fixtures/checksums, wire lifecycle,
containers/reparent, menus, transform gizmo, and Undo/Redo. A final interaction
must be a real on-screen coordinate event.

## Paste-ready cloud runner prompt

```text
Work autonomously in Liesegang/video-editor. First read
docs/cloud-runner-handoff-2026-07-20.md completely, then run git fetch --prune
origin, verify main == origin/main, and verify
aa3a402ecd150adb0ed9f0ce2a682f2d152e2ab5 is an ancestor. Keep the handoff
document commit; do not reset main back to that implementation baseline.

The top architectural objective is a single authoritative Project edited by
Timeline, Inspector, Preview, Renderer, and an independent loosely coupled
Node Editor UI library. Do not add a persisted intermediate/facade model.
Create one workspace crate `node-editor-ui` whose primary API is per-frame
GraphFrame descriptors -> domain-neutral EditorOutput intents. It may own only
transient UI interaction state and must not depend on app or the video library.
Keep all Composition/Track/Clip/Time/FPS/Image/Shape/plugin/history semantics in
a thin video-editor adapter. Preserve the pre-v1 no-migration/no-alias policy.

Delegate bounded investigations and implementations to subagents and run
independent adversarial review before integration. Parallelize independent
work, but merge feature branches into main sequentially. Use small semantic
commits and push them promptly. Never merge a branch just because it compiles.

Start three bounded tracks from the pushed checkpoints:
1. origin/codex/node-editor-modular-38a1579: add the approved ADR, dependency
   checker, node-editor-ui skeleton, fake host tests, then extract generic
   canvas/layout/geometry/interaction/render/QA modules. Preserve wire
   deselection tests and never cache an authoritative duplicate Snarl graph.
2. origin/codex/transform-style-fe333f6: diagnose the still-flaky fresh-spawn
   real-coordinate Node selection E2E, remove all remaining positive line-count
   deltas in oversized files, run current-HEAD full gates and consecutive E2E,
   then independently review before merging.
3. origin/codex/python-expression-foundation: remove the final node_editor and
   inspector/properties line-count growth, run strict/no-libpython/full tests,
   add coordinate QA, adversarially verify fallback-vs-fail-closed contracts,
   then review before merging. Script authoring UI remains a separate required
   feature.

Also implement a no-allowlist Rust file-size ratchet against the integration
base: no new >1000-line Rust file and no growth of an existing oversized file.
The final rule remains every Rust file <=1000; split the recorded 19 violations
to zero and then enable the hard checker unconditionally in quality-gate.sh.

After safe domain branches land, rebase the Node Editor library branch and
resolve their UI changes into the app adapter, not back into a god file. Then
rebase origin/codex/fmod-number-c609c81, finish the shared vertical vector
widget, hard-limit validation, ordering, and coordinate E2E before review and
integration.

Continue through the prioritized task list in the handoff: Clip semantic
query/commands, Timeline-only complete editing, exact Node Inspector,
Backplate two-Shape geometry-only output, ordered blended Merge, bypass and
optional/no-output semantics, dynamic plugin boundaries, Preview profiling,
and the full HTTP coordinate E2E matrix.

Do not claim completion from unit tests alone. For every GUI behavior, resolve
fresh screen rectangles from the HTTP QA bridge and finish with a real
coordinate click/drag/keyboard event, then verify Project/history/render state
on a later frame. Keep main pushable and report branch/commit/test/evidence
after each integration checkpoint. Communicate progress and blockers in
Japanese.
```
