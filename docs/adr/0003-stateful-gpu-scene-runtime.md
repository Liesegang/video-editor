# ADR 0003: Stateful GPU scene runtime

- Status: Accepted; first bounded Particle slice implemented
- Date: 2026-09-04

## Context

The native catalog declares typed Particle and 3D nodes, but those descriptors are placeholders.
There is no executable particle simulation or 3D renderer today. The current Render Plan evaluates
an image at an arbitrary media time, while particles require persistent state, deterministic seeks,
and GPU resources with lifetimes longer than one frame.

RuViE already renders through Skia Ganesh on a glutin OpenGL context. Creating a second device for a
new scene backend would duplicate ownership, synchronization, caches, and failure handling.

## Decision

`SceneRuntime` is the single execution boundary for stateful Particle and 3D Module nodes. Preview,
scrubbing, thumbnails, and export all call this runtime; no editor surface owns a simulator or a
private render cache.

The first backend shares the existing Ganesh OpenGL context and requires OpenGL 4.3 compute shaders
and shader-storage buffers. Raw GPU work uses `glow` 0.16. A scene pass performs this sequence at one
authoritative boundary:

1. flush and submit pending Skia work;
2. save the OpenGL state touched by the scene runtime;
3. dispatch compute and draw work;
4. issue the required memory barriers;
5. restore OpenGL state; and
6. reset the Skia direct context before Skia resumes.

The application reports an actionable unsupported-device diagnostic when the required capability is
missing. It does not present a CPU placeholder as GPU Particle support. A separate wgpu device is not
used because it would create a second graphics authority and an avoidable texture-transfer path.

A Particle Clip is a bounded Node Clip as defined by ADR 0001. Its Module Definition owns emitter,
initialization, force, modifier, material, and renderer topology. Its Timeline Item owns placement,
interval, local time, and keyframes. The simple Inspector edits published parameters and curated
topology operations on that same Definition; opening the production Node Editor reveals and edits the
same graph. There is no second `ParticleClip` settings model to synchronize.

`ParticleSystem` values are compiled transient resources, not serialized particle arrays and not
external binding targets. Stateful resources are addressed by a `SceneInvocationKey` containing the
Instance Path, Module Instance ID, module state-slot identity, and selected output identity. Shared
Module Definitions share compiled pipelines, but never mutable particle state between placements.

## Time and determinism

Simulation advances in fixed 1/120-second steps in host-local time. Emission, forces, and published
automation are sampled at defined step boundaries. Random streams derive from an authored seed,
stable node/state-slot identity, and `SceneInvocationKey`; wall-clock time, thread order, and preview
frame rate never contribute to a result.

A forward request advances from the current state. A discontinuous or backward seek restores the
nearest valid checkpoint at or before the requested local time and deterministically replays fixed
steps. If no checkpoint exists, replay starts from the invocation origin. Checkpoints and live GPU
buffers are derived, bounded cache data and are never persisted in the Project.

Preview and export create independent execution sessions so preview history cannot affect an export.
They share immutable compiled pipelines and decoded resources through the same cache service. Given
the same Project, assets, capability profile, and requested times, both paths execute the same kernels
and scene renderer.

## Compilation and caching

The Render Plan preserves Module invocations instead of expanding the particle graph per instance.
Compilation produces immutable shader pipelines, resource layouts, state-slot declarations, and
dependency metadata. Executable cache keys include the Module executable hash, kernel ABI and backend
version, capability profile, asset fingerprints, and parameters that change allocation or pipeline
layout. Instance values and keyframes that only change uniforms do not recompile a Definition.

The shared runtime owns bounded LRU budgets for live invocations, GPU buffers, pipelines, decoded
scene resources, and seek checkpoints. Invalidating a Module, asset, or capacity dependency destroys
or retires all affected state through this one owner.

## Safety limits

Particle capacity, allocation size, dispatch dimensions, force-stack length, checkpoint count, and
replay work per request are validated against configurable hard limits. Numeric inputs must be finite
and ranges are normalized before reaching a shader. Only trusted built-in kernels or validated plugin
kernels may execute; user-authored strings are not concatenated into unrestricted GLSL. Device loss,
shader compilation failure, resource exhaustion, and replay-budget exhaustion return diagnostics
instead of hanging the UI or silently rendering a different result.

## Project format

The repository is pre-v1. Only graph topology, published interfaces, authored parameters, seeds, and
Timeline automation are project state. GPU handles, compiled shaders, particle buffers, checkpoints,
and runtime capability data are never serialized. No reader, migration, or compatibility simulator
is added for earlier placeholder contracts.

## Consequences

Inspector-only and advanced Node editing remain two depths over one Particle definition. Multiple
placements can reuse compiled work without sharing simulation state, and deterministic seeking makes
stateful results suitable for both interactive editing and offline export.

The first executable implementation is deliberately narrow: Particle Emitter, Initialize Particle,
Gravity Force, Drag Force, and Sprite Renderer compile as one trusted OpenGL 4.3 program and require a
Module Output. Other Particle and 3D descriptors stay unavailable rather than selecting a different
runtime. Simulation-affecting Timeline automation and connected value inputs are rejected until the
derived scene command can carry a deterministic step-sampled schedule; render-only Sprite color may
change without invalidating simulation history. Hardware parity coverage is an opt-in ignored test so
GPU-less CI remains deterministic.
