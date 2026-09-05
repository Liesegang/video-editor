# Production performance baseline

The performance baseline calls the same authoring, RenderPlan, renderer,
audio, and export boundaries used by the application. It is a measurement
command, not a second evaluator or a mocked renderer.

Run the default optimized benchmark from the repository root:

```sh
cargo xtask performance-baseline
```

The default report is
`target/performance/production-baseline.json`. A reviewable report can be
written explicitly:

```sh
cargo xtask performance-baseline \
  --output docs/performance/current-main-windows-x86_64.json \
  --warmup 2 \
  --samples 5
```

`--warmup` may be zero. `--samples` must be greater than zero. Cargo builds
the custom benchmark with the optimized `bench` profile, and `xtask`
validates the emitted JSON before reporting success.

## Measured production paths

The v1 fixture records these operations in nanoseconds per operation:

- file-backed `ProjectFileStore::load`, including deserialize and validation;
- hierarchical `RenderPlanCompiler::compile` for 100, 1,000, and 10,000
  ordinary Timeline items;
- compilation of 1,000 Module placements sharing one compiled definition;
- cold first-frame Preview, discontinuous seek, and 30 consecutive frames;
- one Timeline property edit through `TimelineEditorService` through its next
  rendered Preview frame;
- cold and cached 4,800-frame authoring audio windows;
- production single-frame PNG render, color termination, and file export.

Preview measurements intentionally select the deterministic CPU Skia backend.
Consequently GPU Preview and graphics-driver values are explicitly `null`
with a reason; they are not inferred from the machine. Complete video export
is also explicitly unavailable because its result depends on the configured
external FFmpeg binary, codec, and destination filesystem. The production PNG
export boundary remains measured.

To additionally measure the actual OpenGL Preview renderer at 320×180 and
1920×1080, run:

```sh
cargo xtask performance-baseline --gpu-preview --output target/performance/gpu-preview.json
```

This opt-in workload uses the same four Solid items as the CPU fixture at
each stated resolution. It records frame evaluation separately from warm
rasterization, working-pixel readback, and display color termination. GPU
initialization and the first managed frame are outside the warm timing;
UI texture upload and presentation are not measured. The command rejects a
missing OpenGL context or raster-backed Project working surface instead of
labeling a CPU fallback as GPU performance.
Device and driver strings come from that renderer's current OpenGL context.
Compare like-sized metrics with the same device and concurrent workload;
these short measurements do not establish sustained 60 fps or dropped-frame rates.

The first-frame metric includes RenderPlan compilation, frame evaluation, CPU
Skia renderer construction, and Preview raster. Seek and consecutive-frame
metrics reuse an initialized renderer and compiled plan. The audio fixture
uses the checked-in `test_data/e2e_media/tone.mp3` through the production QA
fixture and mixer.

## Reproducibility and comparison

The report includes:

- operating system, architecture, OS version probe, CPU probe, and logical
  parallelism;
- nullable GPU and driver probes with explicit reasons when unavailable;
- `bench` profile, Rust compiler, Git commit, and dirty-worktree state;
- fixture generator version and SHA-256 identities for both the serialized
  load fixture and audio media;
- warmup count, sample count, every raw sample, and summary statistics.

Only compare reports with the same schema version, fixture SHA-256, profile,
and equivalent machine metadata. A report with `git_dirty: true` is useful for
local diagnosis but is not a release regression authority. Regenerate the
checked-in current-main result from the clean commit that will own the harness.

The command never checks out or resets another revision. Historical-tag
comparison is deliberately not automated: measure a recovery tag only from a
separate clean worktree or clone so the active worktree cannot be changed.

The machine-readable contract is
[`baseline-schema-v1.json`](baseline-schema-v1.json).

The initial Windows capture is
[`current-main-windows-x86_64.json`](current-main-windows-x86_64.json). It
truthfully records `git_dirty: true` because it was produced before this
harness was committed; regenerate it on the clean integration commit before
using it as a regression threshold.
