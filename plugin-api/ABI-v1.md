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

ABI v1 integrates only the `effector` category. If any component in a bundle
declares another category, RuViE rejects the entire bundle before registering
its descriptor or exposing generic invocation. This prevents a component from
appearing installed without a functioning host adapter.

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

The implemented operation is:

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

Do not transport video frames, decoded audio, GPU objects, or other hot-path
resources as JSON. Such categories use `query_extension` with a separately
named/versioned C table and host-owned opaque resource handles or an explicit
pixel/audio buffer contract. No high-bandwidth extension is standardized in
v1 yet.

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
