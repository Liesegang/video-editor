# Random Property runtime plugin

This crate is a native ABI-v1 plugin. It depends only on
`ruvie-plugin-api` and ordinary third-party crates; it never links RuViE's
`library` or application implementation.

Build the `cdylib`, put the platform library beside `ruvie-plugin.toml`, and
install that directory in a configured RuViE runtime-plugin path. The
`random_property` evaluator exposes `amplitude` and `seed`, and returns a
deterministic value for each rounded millisecond time bucket. The same bundle
also exposes descriptor-backed Fill/Stroke Style components and a Backplate
Decorator component so the fixture exercises the negotiated
`decorator.evaluate.v1`/`decorator.evaluate.v2` adapters over the ABI-v1
invocation table. It advertises both operations so a new host selects the
two-Shape v2 contract while an older host can still invoke the frozen v1
contract. Both callbacks resolve the same descriptor properties. The v1
fallback keeps target and padding but necessarily drops offset/fit, does not
consume the authored background Shape, and uses a fixed rounded-rectangle
appearance. The Backplate component configures only target
grouping, padding, offset, and template fit; the host graph supplies arbitrary
background Shape geometry and a separate Style.
The typed Loader accepts separate image and custom-video RGBA fixtures. Its
video fixture verifies that source time, stream selection, and input/output
color-space metadata cross the dynamic ABI intact.

The repository's `scripts/test-runtime-plugin.sh` performs the stronger
post-build proof: it builds RuViE and its probe first, builds this plugin in a
separate target tree afterwards, installs the bundle, and invokes it through
the unchanged host binary. The probe parses an explicit v1 fallback response,
then proves that the new host negotiates v2 by evaluating an explicit runtime
Style/Decorator two-Shape Project graph and rasterizing it with the CPU renderer.
