# Runtime native plugins

RuViE can discover a plugin compiled **after the host application**. The
stable boundary is the `ruvie-plugin-api` crate: plugin implementation code
does not link into `app` or `library`.

## Author workflow

1. Create a standalone `cdylib` crate. Depend on `ruvie-plugin-api`, not on the
   editor's `library` crate. The example is in
   `examples/third-party-opacity-effector`.
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
validated and translated into it. `PluginManager::create_effector_instance`
materializes every declared property in both cases, so callers cannot forget
fields such as `mode` or `target`. Both then use the same plugin ID +
`EffectorInstance` + `PropertyMap` editing model; the Project does not branch
on implementation tier.
If the plugin later becomes unavailable, the Project's plugin ID and property
map remain ordinary serialized data and are preserved; the renderer simply
cannot invoke that implementation until it is installed again.

Run the actual post-build proof with:

```sh
./scripts/test-runtime-plugin.sh
```

The script builds the app and exact probe into an isolated host target first,
independently builds the example plugin into a different target directory,
checks the locked dependency tree contains neither `app` nor `library`,
installs it into a temporary runtime bundle, runs the unchanged prebuilt
probe, and runs the explicitly ignored external integration test with the
bundle environment supplied. That test concurrently rescans the bundle and
uses plugin-side instrumentation to prove the descriptor callback ran exactly
once; it cannot pass by silently skipping when the bundle is absent.
Set `CARGO_TARGET_DIR` to reuse its isolated
`runtime-plugin-e2e/host` and `runtime-plugin-e2e/sample` build caches. When
the variable is absent, both targets live under a self-cleaning temporary
directory.

## ABI and current scope

Plugin ABI version 1 is independent of Project serialization. No Project
schema version or migration is involved. The JSON invocation envelope is a
low-bandwidth control plane, and only Effector output is integrated into
rendering in this version. It is deliberately **not** a frame-transport API.
Effects, loaders, and entity converters need versioned typed extension tables
obtained through `query_extension`, using host-owned opaque frame/resource
handles (or an explicitly specified pixel-buffer contract) and host service
callbacks. Serializing every frame through JSON would be incorrect.

Runtime Style, Decorator, Effect, loader, exporter, property evaluator, and
entity-converter adapters still need implementations. ABI v1 rejects a bundle
containing any of those categories as a whole; it does not retain a
descriptor-only component or expose generic invocation for something the
editor cannot execute. A category becomes legal only together with its real
host adapter. Performance-sensitive built-ins may intentionally remain
statically linked native implementations; externalization is not required.
Any future runtime hot-path adapter must still expose the same plugin
IDs/definitions and editing model, while using a typed/batched ABI rather than
per-frame or per-character JSON.

## Security

Native plugins execute in the RuViE process with the user's permissions. Only
install trusted bundles. A plugin can read files, start processes, corrupt
memory, or crash the host; the C ABI is a compatibility boundary, not a
sandbox. RuViE validates manifest containment, ABI version/table size,
descriptor structure, declared operations, payload size, returned buffer
invariants, exact default-value types/ranges, property metadata, duplicate
IDs, supported categories, and non-finite Effector output.
Those checks cannot make hostile native code safe. Production distribution
should add signatures/trust policy before offering automatic downloads.
