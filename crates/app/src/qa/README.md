# RuViE HTTP QA bridge

The bridge is disabled by default. Start the app with a loopback port:

```sh
RUVIE_QA_PORT=39091 cargo run -p app
```

The server always binds to `127.0.0.1`; the host is intentionally not
configurable.

## Inspect components

```sh
curl -s http://127.0.0.1:39091/health
curl -s http://127.0.0.1:39091/v1/components
curl -s http://127.0.0.1:39091/v1/components/preview.canvas
```

Each component has a stable `id`, semantic `type`, final rectangle in egui
logical points and physical pixels, visibility, enabled state, and frame ID.
Only a complete frame is published, so an HTTP client never sees a partially
rebuilt registry.

Stable production-authoring ID families include:

- `node_editor.canvas`, `node_editor.node:*`, `node_editor.port.*`
- `timeline.track:*`, `timeline.item:*`
- `assets.asset:*`, `assets.module:*`, `assets.node_clip_source`
- `inspector.effect:*`, `inspector.property:*`
- `curve_editor.canvas`, `curve_editor.key:*`, `curve_editor.keyframe_menu.easing.*`
- `preview.canvas`, `preview.content`, `preview.position_gizmo`, `preview.*` controls
- `preview.path.editor`, `preview.vector.point:*`, `preview.vector.handle_*:*`,
  `preview.vector.mode.*`

`node_editor.canvas` is registered after the production Module Snarl finishes
drawing. Its metadata is authoritative for that completed UI frame:

```json
{
  "document_kind": "module_definition",
  "module_definition_id": "...",
  "module_node_count": 2,
  "module_connection_count": 1,
  "scale": 1.0,
  "translation": {"x": 640.0, "y": 360.0},
  "production_surface": "egui_snarl",
  "timeline_graph_expansion": false
}
```

Re-query this component after every zoom or pan. Do not reuse a rectangle or
transform from an earlier frame.

Percent-encode a stable ID when it contains URL-reserved characters.

## Inject pointer input

Coordinates default to egui logical points and can instead use
`"coordinate_space":"pixels"`. Buttons are `primary`, `secondary`, `middle`,
`extra1`, or `extra2`.

```sh
curl -s -X POST http://127.0.0.1:39091/v1/input/click \
  -d '{"x":640,"y":360,"button":"primary"}'

curl -s -X POST http://127.0.0.1:39091/v1/input/double-click \
  -d '{"x":640,"y":360,"button":"primary"}'

curl -s -X POST http://127.0.0.1:39091/v1/input/drag \
  -d '{"from":{"x":300,"y":200},"to":{"x":600,"y":220},"steps":8}'
```

Also available: `/v1/input/move`, `/v1/input/press`, and
`/v1/input/release`. Clicks are emitted as press and release on separate UI
frames. Double-clicks settle the pointer, then emit two real press/release pairs
in the following `RawInput` frame so wall-clock stalls cannot break egui's click
counting. Drags emit press, intermediate pointer moves, and release across
multiple frames, so normal hit testing and gesture lifecycle run.

Inject a real command-modified wheel event at a screen coordinate with
`/v1/input/scroll`:

```sh
curl -s -X POST http://127.0.0.1:39091/v1/input/scroll \
  -d '{
    "x": 640,
    "y": 360,
    "delta_x": 0,
    "delta_y": -10000,
    "coordinate_space": "points",
    "modifiers": {"command": true}
  }'
```

This is the normal egui mouse-wheel path used by the Node Editor zoom test;
there is no QA command for setting the canvas transform directly.

Inject a real root viewport close request with `/v1/input/close-request` and
an empty JSON object. The application still decides whether to emit
`CancelClose` or accept the request:

```sh
curl -s -X POST http://127.0.0.1:39091/v1/input/close-request -d '{}'
```

Inject a native cursor-centered pinch factor with `/v1/input/pinch`:

```sh
curl -s -X POST http://127.0.0.1:39091/v1/input/pinch \
  -d '{
    "x": 640,
    "y": 360,
    "factor": 1.25,
    "coordinate_space": "points"
  }'
```

The factor is multiplicative (`1.0` means no change) and must be between
`0.01` and `100`. The bridge moves the pointer to the supplied coordinate and
injects `egui::Event::Zoom`; it does not write Preview camera state directly.

The bridge first moves the pointer to a click/drag origin on its own UI frame,
then emits press, motion, and release on subsequent frames. This avoids
counting the synthetic approach to a target as drag motion on drag-only egui
widgets.

Input responses return an `action_id`. Poll its injection status with:

```sh
curl -s http://127.0.0.1:39091/v1/actions/1
```

`injected` means the events reached egui's `RawInput`; tests should separately
query the model or component registry to assert the resulting UI state.

## Capture the rendered viewport

Queue a capture with an empty request body, then poll its metadata until the
phase is `ready` (or `failed`):

```sh
curl -s -X POST http://127.0.0.1:39091/v1/captures
curl -s http://127.0.0.1:39091/v1/captures/1
curl -s http://127.0.0.1:39091/v1/captures/1.png -o ruvie-qa.png
```

The POST response is `202 Accepted` with a `capture_id` and `queued` phase.
Metadata contains `requested_frame`, nullable `completed_frame`, `viewport`,
`pixels_per_point`, pixel `width`/`height`, and the SHA-256 of the encoded PNG.
PNG reads return `409 Conflict` while the capture is queued, capturing, or
failed. Unknown and evicted capture IDs return `404 Not Found`.

This endpoint uses eframe's `ViewportCommand::Screenshot` and accepts only the
matching `Event::Screenshot`; it does not read an internal render buffer.
Requests time out after five seconds. At most 16 captures may be active, 64
status records are retained, a capture is bounded to 128 MiB of RGBA input and
64 MiB of PNG output, and retained PNG data is capped at 128 MiB.

## Coordinate E2E suites

The native app accepts deterministic `authoring_e2e`, `authoring_audio_e2e`,
and `authoring_path_e2e` fixtures. Each is built through
`TimelineEditorService`, so it exercises the same Project model as the shipping
UI. There is no legacy Project/container fixture compatibility path.

Build once, start an isolated native process per suite, capture every final
viewport, and retain all evidence under `target/qa-runs`:

```sh
python3 scripts/qa-runner.py --mode smoke
python3 scripts/qa-runner.py --mode full
```

Smoke builds the debug app for short iteration. Full builds and runs the
release app so renderer, encoder, and realtime timing are validated with the
same optimized code path users receive; `--app-binary ... --no-build` may be
used to reuse an already built binary.

The full run covers these production-reachable workflows:

- imported Asset rows remain distinct and drag into Timeline without a button;
- a clip moves and trims independently of overlapping sibling clips;
- Preview renders, plays, pans and zooms with its grid and source-sized gizmo;
- canonical Path clips edit through visible vertices, coupled Bezier handles,
  point modes, Escape cancel, one-step Undo, and a freshly rendered Preview;
- Inspector displays both Effects and edits through the shared numeric control;
- Curve Editor points, interpolation menu, pan, and independent zoom work;
- an explicit Node Clip opens the production Node Editor, where a Node can be
  created, connected, and stress-zoomed without expanding the Timeline.

Each focused script also supports `--spawn`, for example:

```sh
python3 scripts/qa-curve-editor-e2e.py --spawn
python3 scripts/qa-assets-timeline-e2e.py --spawn
python3 scripts/qa-node-editor-e2e.py --spawn
```

All suites use `scripts/qa_support.py` for HTTP input, process lifecycle, and
Windows-safe cleanup. They never mutate the Project through a QA-only command.
