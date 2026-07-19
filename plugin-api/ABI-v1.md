# RuViE native plugin ABI v1

`include/ruvie_plugin_v1.h` is the language-neutral ABI contract. The Rust
types in `src/lib.rs` have the same `repr(C)` layout. The Project format has no
ABI field: native ABI versioning and Project serialization are separate.

## Lifetime, ownership, and threading

- A bundle library exports `ruvie_plugin_entry_v1`. It returns a non-null
  pointer to a static `RuviePluginApiV1` table with `abi_version == 1` and
  `struct_size >= sizeof(RuviePluginApiV1)`.
- `context` is opaque plugin-owned state. RuViE never dereferences it.
- RuViE can call `descriptor_json`, `invoke_json`, and future extension-table
  callbacks concurrently. The table, context, and returned extension tables
  must remain valid until process shutdown. RuViE keeps the library loaded.
- Input `RuvieBytesView` memory is host-owned, immutable, and valid only until
  the callback returns. A plugin must copy anything it retains.
- Every valid output `RuvieBuffer` is plugin-allocated. `ptr/len/capacity`
  describe that allocation. RuViE copies the bytes and calls that same
  library's `free_buffer` exactly once, including status-error responses.
  Empty output is `{NULL, 0, 0}`.
- No exception or panic may unwind across a callback. Catch it in the plugin
  and return `RUVIE_STATUS_PANIC`. Crossing the C boundary is undefined
  behaviour.
- Payloads are limited to 8 MiB by the reference host.

## Descriptor JSON

The descriptor callback returns UTF-8 JSON shaped as follows. Unknown future
JSON fields should be ignored by plugins reading requests; ABI-v1 host
descriptor fields shown here are required unless marked optional/defaulted.

```json
{
  "name": "Vendor package name",
  "vendor": "Vendor",
  "version": "1.2.3",
  "components": [{
    "id": "org.vendor.component",
    "name": "Display Name",
    "category": "effector",
    "group": "Vendor Group",
    "version": "1.2.3",
    "operations": ["effector.evaluate.v1"],
    "properties": [{
      "name": "amount",
      "label": "Amount",
      "ui": {
        "type": "float", "min": 0.0, "max": 100.0, "step": 1.0,
        "suffix": "%", "min_hard_limit": true,
        "max_hard_limit": true
      },
      "default": 100.0
    }]
  }]
}
```

Property UI `type` values are `float`, `integer`, `color`, `text`,
`multiline_text`, `bool`, `vec2`, `vec3`, `vec4`, `dropdown`, and `font`.
Vector types accept a `suffix`; dropdown accepts `options`. Defaults are
strictly decoded and must match the selected UI type:

- `float` is a finite JSON number; `integer` is a JSON integer representable
  as signed 64-bit without wrapping.
- `color` is exactly `{ "r": 0..255, "g": 0..255, "b": 0..255,
  "a": 0..255 }`, with integer channels.
- vectors are objects containing exactly their finite numeric `x`, `y`, `z`,
  and `w` fields as appropriate.
- text, multiline text, dropdown, and font defaults are JSON strings; bool is
  a JSON boolean. `null` is never converted to a sentinel value.
- hard numeric bounds and dropdown membership apply to defaults. Dropdown
  options must be non-empty, non-empty strings, and unique.

ABI v1 integrates the `effector`, `property`, `style`, `decorator`, `effect`,
and `loader` categories. A property
component must declare `property.evaluate.v1` and an `output_default` in its
component descriptor. The default uses the same explicitly tagged value wire
format as evaluation responses and is the host's safe result when invocation
fails. Its tag also declares the component's output type; a successful response
with another value variant is rejected and falls back to this declared value.
Effector, Style, Decorator, and Effect components omit `output_default`.
Loader components also omit it and declare no graph properties because their
inputs come from a typed load request. Low-bandwidth invocation/response
failure produces `no_output` in the graph instead of inventing a render
config. Effect and Loader execution uses the typed tables below.

If any component in a bundle declares another category, RuViE rejects the
entire bundle before registering its descriptor or exposing generic
invocation. This prevents a component from appearing installed without a
functioning host adapter.

## Generic invocation JSON

The base table's JSON invocation is a low-bandwidth control plane:

```json
{
  "component_id": "org.vendor.component",
  "category": "effector",
  "operation": "effector.evaluate.v1",
  "payload": {}
}
```

The implemented operations are:

- category `effector`, operation `effector.evaluate.v1`
- payload:
  `{"time": 1.25, "properties": {"amount": 50.0, "target": "Char"}}`
- response, one of:
  - `{"type":"no_output"}`
  - `{"type":"transform","translate":[x,y],"rotate":r,
    "scale":[x,y],"target":"block|line|char"}`
  - `{"type":"opacity","opacity":50.0,
    "mode":"set|add|multiply","target":"block|line|char"}`

`parts` is intentionally not a legal ABI-v1 target. The host's batched
renderer does not implement it, so accepting it would turn an otherwise valid
plugin response into a render failure. A future implementation requires a new
explicitly versioned contract.

Transform/Opacity evaluation is per authored Effector instance, not per glyph;
RuViE applies the resulting config in its native batched render path.

- category `property`, operation `property.evaluate.v1`
- component descriptor excerpt:

  ```json
  {
    "id": "org.vendor.random",
    "category": "property",
    "operations": ["property.evaluate.v1"],
    "properties": [],
    "output_default": {"type":"number", "value":0.0}
  }
  ```

- payload:

  ```json
  {
    "time": 1.25,
    "fps": 30.0,
    "properties": {
      "amplitude": {"type":"number", "value":2.0},
      "seed": {"type":"integer", "value":7}
    }
  }
  ```

- response:
  `{"value":{"type":"number","value":-0.75}}`

Property values are always tagged. The v1 variants are `number`, `integer`,
`string`, `boolean`, `vec2`, `vec3`, `vec4`, and `color`. Scalars use a
`value` field, vectors use their finite `x`/`y`/`z`/`w` fields, and color uses
integer `r`/`g`/`b`/`a` channels in `0..=255`. Array/map values and non-finite
numbers are not part of the v1 contract.

The request contains only the evaluator instance's declared property values,
time, and fps. ABI v1 deliberately does not expose RuViE's sibling Project
`PropertyMap` or any internal Project type. A property evaluator that needs
cross-property/project traversal requires a future explicitly versioned host
service extension rather than relying on RuViE implementation layouts.

- category `style`, operation `style.evaluate.v1`
- request:

  ```json
  {
    "time": 1.25,
    "fps": 30.0,
    "properties": {
      "color": {"type":"color","r":255,"g":128,"b":0,"a":255},
      "width": {"type":"number","value":2.0}
    }
  }
  ```

- response, one of:
  - `{"type":"no_output"}`
  - `{"type":"fill","color":{"r":255,"g":128,"b":0,"a":255},
    "offset":0.0}`
  - `{"type":"stroke","color":{"r":255,"g":255,"b":255,"a":255},
    "width":2.0,"offset":0.0,"cap":"round","join":"miter",
    "miter":4.0,"dash_array":[3.0,2.0],"dash_offset":0.0}`

Fill and Stroke are the complete set of host `DrawStyle` variants at the time
this operation was defined. Every numeric field must be finite and remain
finite when converted to the host renderer's 32-bit scalar. Stroke width and
miter must be non-negative. Text's effective width
`max(0, width + 2 * offset)` must remain a finite renderer scalar. For a
non-zero Shape offset, both `2 * (abs(offset) + width / 2)` and the positive
inner width `2 * (abs(offset) - width / 2)` must also remain finite.

An empty dash array means a solid stroke. A non-empty dash array must have an
even length no greater than `RUVIE_STYLE_MAX_DASH_INTERVALS_V1` (1024), every
interval must remain strictly positive as a 32-bit renderer scalar, and the
32-bit period sum must remain finite and positive. The complete pattern and
phase must be accepted by Skia's dash constructor; otherwise the response is
`no_output`, never a silently solid stroke. Cap values are `round`,
`square`, and `butt`; join values are `round`, `bevel`, and `miter`. The host
assigns the resulting config the authored operation Node's ID; plugins neither
receive nor choose Project identity.

- category `decorator`, operation `decorator.evaluate.v1`
- request has the same `time`, `fps`, and explicitly tagged resolved
  `properties` shape as Style.
- response, one of:
  - `{"type":"no_output"}`
  - `{"type":"backplate","target":"block","shape":"rounded_rect",
    "color":{"r":0,"g":0,"b":0,"a":255},
    "padding":{"top":4.0,"right":6.0,"bottom":4.0,"left":6.0},
    "corner_radius":3.0}`

Backplate is the complete set of host `DecoratorConfig` variants at the time
this operation was defined. Targets are `block`, `line`, and `char`; `parts`
is intentionally absent because the host renderer does not implement it.
Shapes are `rect`, `rounded_rect`, and `circle`. Every padding field must be
finite, and the signed horizontal (`left + right`) and vertical
(`top + bottom`) sums that contribute to the padded width and height must
remain finite 32-bit values. The host also applies the padding to a finite,
non-zero reference rectangle (`left=-1`, `top=-2`, `right=3`, `bottom=4`) and
requires all four resulting coordinates and both raw spans to remain finite.
Negative padding is allowed when those checks pass. These config-only checks do
not make non-finite or extreme source bounds safe; source geometry must
independently satisfy the renderer's finite-value precondition. Corner radius
must be non-negative, and both it and its diameter must remain finite 32-bit
values.

Style and Decorator requests contain only resolved declared properties and
scalar metadata. They never transport source shapes, raster frames, paths,
Project types, or renderer/GPU state. Unknown output variants or fields,
non-finite values, invalid enum strings, and fields that violate the rules
above are rejected as `no_output`; they are never partially adapted.

## Typed CPU RGBA8 extensions

Frames never cross `invoke_json`. ABI v1 standardizes these exact
`query_extension` names:

- `ruvie.effect.cpu-rgba8.v1` returns `RuvieEffectCpuRgba8ApiV1` and backs an
  Effect component declaring `effect.process.cpu-rgba8.v1`.
- `ruvie.loader.cpu-rgba8.v1` returns `RuvieLoaderCpuRgba8ApiV1` and backs a
  Loader component declaring both `loader.open.v1` and
  `loader.load.cpu-rgba8.v1`.

Both returned tables start with ABI version and table size, remain
plugin-owned/static while the library is loaded, and contain a thread-safe
opaque context. A bundle that declares either category without its complete
typed table is rejected before any component is registered.

### Shared frame and result contract

RGBA8 v1 is straight (unpremultiplied) alpha and sRGB only. Width and height
must be in `1..=32768`; stride is at least `width * 4`; and buffer length is
exactly `stride * height`, no greater than 512 MiB. Hosts check every
multiplication, dimension, stride, length, alpha-mode, color-profile, table
size, and pointer/capacity invariant before reading pixels. Fully transparent
RGB is canonicalized by the host after copying.

Plugin outputs are `RuvieOwnedRgba8FrameV1`. The host copies a valid frame and
calls that same extension table's `free_frame` exactly once. It also calls
`free_frame` for structurally reclaimable output that is rejected for bad
metadata. An unreclaimable pointer/length/capacity tuple is reported but cannot
be safely passed to plugin deallocation. Inputs are borrowed host-owned views
valid only for the callback.

Typed callbacks return `RuvieExtensionResultV1`. Its optional message is a
plugin-owned `RuvieBuffer` reclaimed through the base table's `free_buffer`.
`RUVIE_STATUS_UNSUPPORTED` means the implementation deliberately declines the
request; `RUVIE_STATUS_PLUGIN_ERROR`, `RUVIE_STATUS_INVALID_REQUEST`, and
`RUVIE_STATUS_PANIC` are real failures and retain the plugin message. No panic
or exception may unwind through any extension callback.

### Effect

`create_instance` receives the component ID plus a borrowed array of resolved,
explicitly tagged properties. The tags cover number, integer, string, boolean,
vec2/3/4, and color. It returns a non-zero opaque handle. The host caches these
immutable handles in a bounded LRU whose identity is the Effect component,
operation, and exact resolved config; eviction/drop calls `release_instance`
once. Concurrent cache misses may create a duplicate, but the unused handle is
released rather than leaked.

`process` receives only that handle, finite evaluation time, and a typed
borrowed frame. There is no per-frame JSON and no Project/renderer/GPU object.
The current host adapter accepts CPU `Image` input; a GPU texture must first be
materialized by a future explicit contract rather than exposing an internal
handle accidentally.

### Loader

`open` receives UTF-8 component/path views and writes at most 64
`RuvieAssetMetadataV1` records into host-owned memory. Presence bits distinguish
absent duration, FPS, dimensions, stream index, frame count, and time base.
The host rejects unknown bits, non-finite/invalid values, oversized dimensions,
and zero time-base terms.

`load` receives a typed image or video-frame request containing path,
source-local time, optional stream index, and optional color-space names. A
successful callback returns the same owned RGBA8 frame contract. Loader cache
identity includes plugin/component operation, source path/file identity,
stream/color config, and exact source-time bits for video. The manager clones
the selected plugin `Arc` and releases registry locks before `open`, `load`, or
Effect callbacks. `Unsupported` permits trying the next loader; once a loader
claims a request and returns a failure, its component ID, path, and original
cause are returned instead of a misleading “no plugin registered” error.

Do not transport decoded audio, GPU objects, Project objects, or other
hot-path resources as JSON. They require another explicitly named/versioned
typed table with equally explicit ownership.

## Bundle manifest

The library and `ruvie-plugin.toml` live in one bundle directory. Library
paths cannot escape that directory.

```toml
manifest_version = 1

[library]
macos = "libplugin.dylib"
linux = "libplugin.so"
windows = "plugin.dll"
```

Distribute separate binaries/bundles for each supported OS and architecture.
Native plugins are trusted in-process code and are not sandboxed.

RuViE resolves and atomically claims the canonical manifest/library identity
before `dlopen` or either plugin callback. Concurrent scans report the bundle
as in-flight or already loaded; they never call the entry/descriptor callback
a second time. Loaded native libraries remain loaded until process exit.
