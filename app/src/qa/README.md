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

Initial stable ID families are:

- `node_editor.container.*`, `node_editor.node:*`, `node_editor.port.*`
- `timeline.track:*`, `timeline.clip:*`
- `inspector.property.*`, `inspector.keyframe.*`
- `graph.canvas`, `graph.ruler`, `graph.keyframe.*`
- `keyframe_dialog.*`
- `preview.canvas`

`node_editor.canvas` is re-registered after Snarl finishes drawing. Its final
metadata is therefore authoritative for that completed UI frame:

```json
{
  "composition_id": "...",
  "scale": 0.0065,
  "translation": {"x": 640.0, "y": 360.0},
  "min_scale": 0.0065,
  "max_scale": 1.25,
  "detail_enabled": false,
  "port_interaction_enabled": false,
  "resize_interaction_enabled": false
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

curl -s -X POST http://127.0.0.1:39091/v1/input/drag \
  -d '{"from":{"x":300,"y":200},"to":{"x":600,"y":220},"steps":8}'
```

Also available: `/v1/input/move`, `/v1/input/press`, and
`/v1/input/release`. Clicks are emitted as press and release on separate UI
frames. Drags emit press, intermediate pointer moves, and release across
multiple frames, so egui's normal hit testing and drag lifecycle run.

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

Run the keyframe editing suite against a fresh deterministic fixture:

```sh
python3 scripts/qa-keyframe-e2e.py --spawn
```

It adds, updates, and removes Inspector keyframes across Direct, Effect,
Style, Effector, and Decorator targets; performs a multi-point Graph Editor
drag; opens the keyframe dialog; and compares the authoritative Project,
Inspector metadata, Preview pixels, and one-entry-per-gesture history.

Run the focused 100x Node Editor navigation suite with:

```sh
python3 scripts/qa-node-editor-e2e.py --spawn
```

It resolves the latest `node_editor.canvas` center, injects a real
command-wheel zoom to the `0.0065` clamp, re-queries geometry, then performs a
real primary-coordinate drag. It requires scale and all LOD gates to remain
unchanged during pan, verifies the translation delta, and confirms that the
authoritative Project and undo history did not change. The suite is also part
of `python3 scripts/qa-runner.py --mode full`.
