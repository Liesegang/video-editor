# Bounded vector surfaces, 2026-09-06

Small Text and Shape layers previously allocated a full-target transient
Surface even when the body and Drop Shadow occupied only a small rectangle.
The production renderer now crops that allocation to transformed visual
bounds, clipped to the current composition target. It retains an integer
origin and two device pixels of guard space, then composites without image
resampling. Dissolve keeps its target-space noise coordinates.

The existing vector builders still own the paint operation. External Image
Effects continue to receive full-target raster images; the crop does not
change their coordinate contract. Final output and composition group surfaces
also remain full-sized.

## Warm Preview measurements

The fixture contains descriptor-backed Fill and Drop Shadow on 72px Text or
128x72 Shape layers, placed through the ordinary Timeline model in a
3840x2160 composition. Each sample renders the same pre-evaluated frame through
`RenderService<SkiaRenderer>::render_authoring_frame(Preview)`. It is a real
rerender, not a cached vector frame. GPU drawing, terminal color processing,
RGBA8 readback, and result disposal are timed; RenderPlan compilation, frame
evaluation, initialization, and UI upload/presentation are not.

Optimized bench profile, two warmups and five samples, one render per sample.
Both reports use NVIDIA GeForce RTX 3090, driver 616.56, and fixture SHA-256
`2366799e4b319bffcbf1674f5cbbb09e8691cccef4b78417754d29cc79542827`.

| 4K workload | Before median | After median |
| --- | ---: | ---: |
| 1 small Text + Shadow | 20.28 ms | 19.01 ms |
| 16 small Text + Shadow | 27.38 ms | 25.69 ms |
| 1 small Shape + Shadow | 19.86 ms | 19.19 ms |
| 16 small Shape + Shadow | 24.01 ms | 21.11 ms |

Raw emitted reports: [before](vector-surface-before-2026-09-06.json),
[after](vector-surface-after-2026-09-06.json).
Both truthfully record a dirty worktree based on `4b23bfb`.
The before capture had the benchmark additions but no production crop changes;
the after capture included the crop. CPU build/test work ran during the
after measurement period, while native QA and other agent GPU work were
excluded. These are local diagnostic observations, not controlled clean-release
regression thresholds or statistical proof of a 3–12% speedup.

The before report's styled `production_path` label incorrectly lists setup
compilation and frame evaluation alongside the timed renderer call.
The after report corrects that styled label, but its two Solid warm-render
labels contain the same overbroad setup chain. All warm-render timings exclude
setup. The current harness corrects both labels; the paired timing closure
and fixture are unchanged, and neither raw JSON was rewritten after emission.

## Correctness and limits

CPU differential tests compare the native and full-target boundaries across
all 29 blend modes, fractional affine transforms, nested target clipping, and
empty/offscreen layers with a channel tolerance of 0.000001. Real-builder
tests assert that small Text, Shape, and SkSL actually allocate smaller
surfaces, while the corresponding raster boundaries retain target dimensions.

GPU tests cover native/full-boundary output and CPU/GPU layer-mask rendering.
Fractional contours differ between Skia backends even before a blur is added.
That test propagates measured baseline AA contour support through the shadow
offset and blur kernel, compares pixels outside it with the existing 0.002
tolerance, and retains whole-image alpha bounds and channel-energy checks.
Actual wrong-offset and wrong-blur renders are rejected. This is not a claim
of CPU/GPU equality at every contour pixel.

No transient-allocation byte counter is available, so the reports do not
present calculated surface sizes as measured GPU allocation bytes. The
remaining 19–26ms warm frame times already exceed a 16.67ms budget before UI
upload or presentation. Sustained 60fps, large-project memory use, and the
full VJ workload remain open in the project backlog.

The final integration passed 1,680 workspace tests (17 explicitly ignored),
three separately run desktop-GPU tests, strict workspace Clippy, formatting,
and all 24 native HTTP UI scenarios on the release app. The latter includes
Appearance editing, explicit Node Clip conversion, fresh-process persistence,
Preview, and Export; evidence is in
`target/qa-runs/20260906T-vector-surface-final`.
The final grouped-Backplate regression fixes painting to use the same
authoritative parts bounds as allocation; the benchmark fixtures contain no
Backplate and are unaffected by this final correction.
