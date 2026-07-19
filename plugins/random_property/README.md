# Random Property runtime plugin

This crate is a native ABI-v1 plugin. It depends only on
`ruvie-plugin-api` and ordinary third-party crates; it never links RuViE's
`library` or application implementation.

Build the `cdylib`, put the platform library beside `ruvie-plugin.toml`, and
install that directory in a configured RuViE runtime-plugin path. The
`random_property` evaluator exposes `amplitude` and `seed`, and returns a
deterministic value for each rounded millisecond time bucket. The same bundle
also exposes descriptor-backed Fill/Stroke Style components and a Backplate
Decorator component so the fixture exercises every low-bandwidth config
adapter supported by ABI v1.

The repository's `scripts/test-runtime-plugin.sh` performs the stronger
post-build proof: it builds RuViE and its probe first, builds this plugin in a
separate target tree afterwards, installs the bundle, and invokes it through
the unchanged host binary. The probe evaluates an explicit runtime
Style/Decorator Project graph and rasterizes it with the CPU renderer.
