# Preview rendering investigation — 2026-09-05

The reported severe slowdown was reproduced with four full-frame Solid items.
At 1920×1080, frame evaluation took about 0.006 ms, while the OpenGL renderer
and CPU display termination took about 453 ms per frame.

## Fixes in the production path

- Text, Shape, and SkSL without object effects now keep their transient Skia
  Surface native through compositing. The raster drawing code is shared with
  the existing owned-output effect boundary; no second renderer was introduced.
- Builtin color image transforms use the existing global Rayon pool, retaining
  the same f64 transform math, finite validation and deterministic first error.
- Terminal RGBA8 packing consumes transformed RGB directly instead of allocating
  another full RGBAF32 image (33,177,600 bytes at 1920×1080).
- The existing timing logger now includes terminal color processing.

After only the first change, 1920×1080 still took about 294 ms. Timing traces
at that point attributed 238–240 ms to CPU terminal color and about 20–27 ms
to final working-pixel readback. Thus removing per-layer readbacks alone was
insufficient; color conversion was the dominant remaining cost.

## First optimization pass

Optimized bench profile, two warmups, five samples; each warm raster sample
contains three renders. Same machine and four-Solid fixture; no background
build or native QA was running during the final measurements.

| Resolution | Before (median) | After (median) | Speedup |
| --- | ---: | ---: | ---: |
| 320×180 | 12.49 ms | 2.87 ms | 4.35× |
| 1920×1080 | 452.62 ms | 64.75 ms | 6.99× |

Raw reports: [before](gpu-preview-before-2026-09-05.json),
[after](gpu-preview-after-2026-09-05.json).
The final probe verifies both the active OpenGL context and GPU-backed Project
working surface, recording NVIDIA GeForce RTX 3090 and OpenGL driver 616.56.
The initial probe checked the context but predates the surface-backing and
driver metadata checks. Both reports truthfully record a dirty worktree; they
are investigation evidence, not clean-release regression thresholds.

The smaller historical CPU baseline did not reproduce a recent slowdown:
the recorded 11.50 ms became 10.72 ms before these optimizations. The issue
was exposed by adding an actual-device, full-resolution workload.

## Verification and remaining work

CPU and real-GPU comparisons cover Shape, Text and SkSL, transforms, opacity,
layering and non-Normal blending. Color tests compare scalar and parallel
results exactly across standard spaces and supported PQ/HLG boundaries, and
compare fused terminal packing with the previous float-output packing path.
Existing effect processing and color authority remain intact.

These metrics exclude UI texture upload/presentation, video decoding, audio,
and nontrivial effects. This first pass did **not** establish sustained 60 fps:
full-HD 64.75 ms remained above the 16.67 ms budget.

## Follow-up: GPU terminal color

The existing ProjectColorPipeline now authorizes a complete GPU terminal chain
only when each stage has the exact identity of its CPU processor. Builtin GLSL
is generated from the same compiled transfer functions, coefficients, primary
matrices and luminance context as the CPU implementation. Unsupported complete
chains (including the current OCIO named-view backend) keep their CPU processor.

SkiaRenderer executes this terminal stage under its existing OpenGL owner.
Working float pixels remain on the GPU through unpremultiplication, ordered
color transforms, finite/domain validation, alpha-zero canonicalization and
straight RGBA8 packing. Only RGBA8 and a first-invalid-pixel index are read back.
An invalid sample or execution failure rejects the frame instead of publishing
partial pixels. Shader linking and GL state isolation are shared with the
existing Particle runtime; no second device or parallel renderer was added.

The same four-Solid fixture, two warmups and five samples produced:

| Resolution | CPU terminal pass | GPU terminal pass | Original / latest |
| --- | ---: | ---: | ---: |
| 320×180 | 2.87 ms | 0.43 ms | 29.1× |
| 1920×1080 | 64.75 ms | 5.34 ms | 84.8× |

[Raw GPU terminal report](gpu-terminal-after-2026-09-05.json) records the
actual device/driver, source revision and dirty state. The benchmark rejects
both raster fallback and CPU terminal fallback. It waited for the outstanding
release build before running; the user's application was not controlled or
restarted, so this is investigation evidence, not a clean desktop-idle release
regression threshold.

Real-GPU tests compare CPU/GPU terminal output within one RGBA8 level, using
nonuniform images and transparency, multiple stage order, odd sizes and resize,
program reuse and foreign working-identity rejection. NaN is rejected and a
subsequent valid frame succeeds. These renderer-only times fit a 60-fps budget,
but **do not prove end-to-end 60 fps** with video decode, effects, UI upload,
audio, multiple Particle layers, or a user's complete project. Those workloads
remain required performance acceptance work.
