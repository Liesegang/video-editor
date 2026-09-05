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

## Measured results

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
and nontrivial effects. They do **not** establish sustained 60 fps: full-HD
64.75 ms is still above the 16.67 ms budget. The next rendering task is a
ProjectColorPipeline-authorized GPU terminal transform, retaining working
pixels on the device until final RGBA8 output. Backends without a complete
GPU transform chain must retain their exact CPU processor; bypassing color
management is not an optimization.
