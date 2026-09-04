# Third-party Opacity Effector

This example is intentionally outside the RuViE Cargo workspace. It depends
on `ruvie-plugin-api` and `serde_json`, never on the host `app` or `library`.
It demonstrates descriptor-owned `opacity`, `mode`, and `target` defaults and
an Effector invocation through the stable C ABI.

The sample also exposes a low-bandwidth `example.descriptor_calls.v1`
operation used only by RuViE's integration proof. Concurrent rescans query it
to verify that manifest/library identity is claimed before the descriptor
callback executes.

Build it independently:

```sh
cargo build --manifest-path examples/third-party-opacity-effector/Cargo.toml --locked
```

Copy the resulting platform library next to `ruvie-plugin.toml`, then copy the
bundle directory into a configured RuViE plugin directory. RuViE scans those
directories at startup. In Settings → Plugins → Paths, **Rescan Runtime
Plugins** discovers bundles installed while the application is already
running. Restart RuViE to replace a plugin binary that has already been
loaded; libraries remain loaded to keep callbacks valid.

The exported symbol is `ruvie_plugin_entry_v1`. Both callbacks use
`catch_unwind`; no panic may cross the C boundary. Callback results are
allocated by the plugin and reclaimed only by that plugin's `free_buffer`.
The manifest provides OS-specific names for `.dylib`, `.so`, and `.dll`; make
separate bundle builds for each supported OS and CPU architecture.

Native plugins are trusted in-process code, not sandboxed extensions. See
[`docs/runtime-plugins.md`](../../docs/runtime-plugins.md) and the
language-neutral [`crates/plugin-api/ABI-v1.md`](../../crates/plugin-api/ABI-v1.md).

The full proof builds the host first and executes that same prebuilt host-side
probe only after independently building and installing this library:

```sh
./scripts/test-runtime-plugin.sh
```
