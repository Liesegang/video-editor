# Runtime native plugins

RuViE can discover a plugin compiled **after the host application**. The
stable boundary is the `ruvie-plugin-api` crate: plugin implementation code
does not link into `app` or `library`.

## Author workflow

1. Create a standalone `cdylib` crate. Depend on `ruvie-plugin-api`, not on the
   editor's `library` crate. Examples are in
   `examples/third-party-opacity-effector` and `plugins/random_property`.
2. Export `ruvie_plugin_entry_v1`, returning a static
   `RuviePluginApiV1` table. Return descriptor and invocation payloads as JSON
   in plugin-owned `RuvieBuffer` values. Catch every panic before returning
   through the C ABI. The host calls the plugin's `free_buffer` callback for
   every valid returned buffer.
3. Put the platform library beside `ruvie-plugin.toml`:

   ```toml
   manifest_version = 1

   [library]
   macos = "libmy_plugin.dylib"
   linux = "libmy_plugin.so"
   windows = "my_plugin.dll"
   ```

4. Put that bundle directory inside one of the configured plugin paths and
   start RuViE. `PluginManager::rescan_runtime_plugins` can discover bundles
   added after startup. A loaded path is not unloaded or hot-replaced; restart
   the process to replace an existing binary safely. The scanner claims the
   canonical manifest/library identity before loading native code. Concurrent
   scans report an in-flight or already-loaded bundle without invoking its
   entry or descriptor callback again. A later rescan of an already-loaded
   manifest remains idempotent even if its installed binary has been removed
   or damaged.

Each implementation's definitions are the sole source for an instance's
PropertyDefinitions and defaults: built-in native plugins return the same
host `PropertyDefinition` type directly, while runtime native descriptors are
validated and translated into it. Descriptor-backed operation factories
materialize every declared property in both cases, so callers cannot forget
fields such as `mode` or `target`. Both then use the same plugin ID + explicit
operation Node + `PropertyMap` editing model; the Project does not branch on
implementation tier. Runtime Style uses the built-in typed ports. A Decorator
advertising `decorator.evaluate.v2` uses the built-in two-Shape ports, while a
v1-only Decorator retains its frozen one-Shape appearance contract. Plugins
that support both hosts should advertise ordered operations
`[decorator.evaluate.v1, decorator.evaluate.v2]` and implement both callbacks
over one descriptor property set. New hosts negotiate v2 regardless of
operation order; old hosts can still invoke v1. Runtime Effect uses the same
Image input/output operation Node and descriptor-default factory as a built-in
Effect.
If the plugin later becomes unavailable, the Project's plugin ID and property
map remain ordinary serialized data and are preserved; the renderer simply
cannot invoke that implementation until it is installed again.

Runtime property evaluators follow the same rule. Their descriptor defines
the evaluator's configuration properties and an explicitly typed
`output_default`. `PluginManager::create_property_instance` materializes every
configuration default, while sparse loaded data is completed only in-memory
during evaluation. Invocation failures are logged with component ID and cause;
the adapter then returns the descriptor-declared fail-safe instead of silently
inventing a value. ABI v1 sends only time, fps, and evaluator-local properties;
it does not expose the Project's sibling `PropertyMap` or internal Rust types.

Run the actual post-build proof with:

```sh
./scripts/test-runtime-plugin.sh
```

The script builds the app and exact probe into an isolated host target first,
then independently builds the standalone fixture plugin into a different
target directory. That bundle contains Property, Fill/Stroke Style, Backplate
Decorator, typed CPU RGBA8 Effect, and typed CPU RGBA8 Loader components. The
script checks the locked dependency tree
contains neither `app` nor `library`, installs the bundle, and runs the
unchanged prebuilt probe. The probe executes an explicit geometry graph with
`Text + background Shape -> runtime Backplate -> runtime Fill -> runtime Effect`
through the Frame evaluator and CPU renderer, then verifies the exact
post-Effect visible pixel family. Backplate transports fitted Shape geometry
only; its background Shape and downstream Style remain ordinary editable
Project Nodes. Before exercising that negotiated v2 path, the prebuilt probe
also invokes `decorator.evaluate.v1` directly and parses the response through
the frozen v1 output type. This proves the later-installed dual plugin remains
callable by the legacy control plane. The probe also loads image and custom-video fixture
bytes through the post-build Loader by path. The video proof traverses the
native Image and FFmpeg fallthrough before validating source time, stream
selection, color-space names, video metadata, and pixels inside the dynamic
plugin. Corrupt image and video fixtures retain their path and decoder cause.
The explicitly ignored external integration test concurrently rescans the
bundle and uses plugin-side instrumentation to prove the descriptor callback
ran exactly once; it cannot pass by silently skipping when the bundle is
absent.
Set `CARGO_TARGET_DIR` to reuse its isolated
`runtime-plugin-e2e/host` and `runtime-plugin-e2e/sample` build caches. When
the variable is absent, both targets live under a self-cleaning temporary
directory.

## ABI and current scope

Plugin ABI version 1 is independent of Project serialization. No Project
schema version or migration is involved. The JSON invocation envelope remains
a low-bandwidth control plane. ABI v1 integrates Effector output, property
evaluation, low-bandwidth Style/Decorator config evaluation, and separately
typed CPU RGBA8 Effect/Loader hot paths. Runtime Style covers every current
host `DrawStyle` variant (Fill and Stroke). The frozen
`decorator.evaluate.v1` Backplate keeps its original target/shape/color/padding/
corner-radius output and one-Shape paint path. The negotiated
`decorator.evaluate.v2` Backplate is preferred when advertised and carries
only target grouping, padding, offset, and fit for the two-Shape geometry path.
Dual advertisement is recommended for plugins distributed to mixed host
versions. Both operations receive the descriptor's resolved property map. A
v1 fallback must document fields it cannot represent; the sample retains
target and padding, emits a fixed rounded-rectangle appearance, and explicitly
drops v2-only offset and fit semantics. The v2 result remains lossless and is
always selected by a supporting host.
This negotiation preserves component invocation compatibility, not serialized
graph interchangeability. A legacy one-Shape graph has no authored background
input for a new host's negotiated v2 operation, while a two-Shape v2 graph
cannot run unchanged on an old host. There is deliberately no Project-schema
migration for this pre-v1 model. A plugin that needs saved-graph compatibility
must keep its v1 component ID on the one-Shape contract and publish v2 under a
distinct component ID; otherwise the user must explicitly re-author the graph.
Backplate `Parts` is not advertised until authored path-part semantics are
standardized. Descriptor properties are resolved and
validated once, then materialized as constants before plugin evaluation so a
stateful authored evaluator cannot be invoked twice. Invalid, unknown,
non-finite, or structurally unsafe config responses produce `NoOutput`.

Style and Decorator requests contain only time, fps, and explicitly tagged
values for the component's declared properties. They do not contain a shape,
path, Project object, frame, renderer, or GPU object. The host assigns the
resulting config the source operation Node's identity.

Effect and Loader use the exact `ruvie.effect.cpu-rgba8.v1` and
`ruvie.loader.cpu-rgba8.v1` extension tables obtained through
`query_extension`. Resolved Effect properties create an opaque cached instance;
the per-frame callback receives only the handle, time, and a bounded typed
frame. `u_time` is reserved for that transport and is rejected as an authored
Effect property, so time changes never churn cached configuration handles.
Loader metadata is written to bounded host-owned records, while decoded pixels
use plugin-owned frames with an explicit `free_frame`. Loader cache keys include
canonical path and platform file identity, and metadata-open failures preserve
the claiming plugin's path and cause. Both contracts validate aligned tables,
dimensions, checked stride/length, straight alpha, sRGB, and callback status.
They never pass frames through JSON. Exporter, entity-converter, GPU, and audio
hot paths still need their own versioned typed extensions.

A category becomes legal only together with its real host adapter.
Performance-sensitive built-ins may intentionally remain statically linked
native implementations; externalization is not required. Future hot-path
adapters must still expose the same plugin IDs/definitions and editing model
while using a typed/batched ABI rather than per-frame or per-character JSON.

## Security

Native plugins execute in the RuViE process with the user's permissions. Only
install trusted bundles. A plugin can read files, start processes, corrupt
memory, or crash the host; the C ABI is a compatibility boundary, not a
sandbox. RuViE validates manifest containment, ABI version/table size,
descriptor structure, declared operations, payload size, returned buffer
invariants, exact default-value types/ranges, property metadata, duplicate
IDs, supported categories, and non-finite or structurally unsafe
Effector/Property/Style/Decorator output, and bounded Effect/Loader RGBA8
metadata and buffers.
Those checks cannot make hostile native code safe. Production distribution
should add signatures/trust policy before offering automatic downloads.
