# ADR 0004: Timeline 3D space and cameras

- Status: Accepted; implementation is staged
- Date: 2026-09-04

## Context

RuViE currently has two-dimensional Timeline transforms and placeholder `Camera3D`, `Object3D`, and
`Render3D` node contracts. A useful 3D editor needs camera cuts, parented transforms, keyframes,
gizmos, particles, and imported meshes without converting every Timeline Item into a Node.

Introducing a separate scene document would duplicate placement and automation ownership. Rendering
every 3D clip independently would also prevent correct depth interaction and camera control.

## Decision

Timeline remains the authority for composition-level space and time. A visual Timeline Item has one
canonical spatial transform with position, anchor, scale, and rotation. Two-dimensional editing is a
constrained view of that transform; enabling 3D exposes the Z and X/Y rotation channels instead of
creating a second transform. The Timeline, Dope Sheet, Curve Editor, Inspector, and Canvas gizmo all
edit the same stable property and keyframe identities.

Authoring coordinates are screen-compatible and right-handed: +X is right, +Y is down, and +Z points
away from the viewer. A default camera therefore sits on negative Z and looks toward positive Z.
Rotation is authored as independent X/Y/Z degree channels and converted in a fixed documented order
to the matrix used by evaluation. Parent and child transforms are composed as 4x4 matrices, including
when a 2D plane participates in a 3D hierarchy.

A Camera is a first-class, non-imaging Timeline source with an interval, transform, projection mode,
focal length or field of view, near/far planes, and focus controls. At a given composition-local time,
the highest enabled active Camera Item in layer order is authoritative. If none is active, the
Composition uses its deterministic default camera. Camera cuts are ordinary interval and layer edits;
camera properties use ordinary Timeline automation.

Node Modules may construct objects, cameras, and procedural scenes internally. Outside a Module,
only a published typed interface can supply or receive such a value. Timeline automation targets a
published parameter, never an internal transform-node UUID. A Module-local camera does not silently
replace a Composition camera.

## Composition and rendering

The Render Plan retains Composition hierarchy and compiles explicit scene passes. Each maximal
contiguous run of active 3D Timeline Items is depth-tested together under the selected Composition
camera. A 2D item, matte, or explicit flattening operation is a compositing boundary, so layer order
remains deterministic. Flattening is an execution decision and does not change the authored model.

An ordinary image, text, shape, video, or Node Clip can become a camera-facing or freely rotated plane
without being expanded into Nodes. Scene-producing Node Clips can contribute typed scene content to a
scene pass; image-producing Node Clips remain normal composited planes. Bounds, picking, masks, and
Canvas gizmos derive from the evaluated object geometry and spatial transform, not the Composition
rectangle.

Nested Timelines retain their own local origin, camera selection, and time mapping. They render as a
flattened plane by default. Entering or exposing their 3D space is an explicit authoring choice with a
defined Render Plan dependency; moving the outer instance never rewrites inner keyframes.

## Delivery order

The first executable slice is a self-contained 3D Node Clip using ADR 0003: an internal camera,
emitter or mesh, transforms, forces, and renderer produce an Image output, with published controls in
the Inspector. This validates the shared GPU runtime without prematurely creating a Timeline-wide
scene model.

Timeline-wide 3D follows with canonical spatial properties, Camera Items, 3D parenting, scene-pass
compilation, and common Timeline/Dope Sheet/Curve editing. Both slices use the production Node Editor,
shared viewport controller, shared Scene Runtime, and shared caches. Placeholder catalog entries are
not evidence that either slice is implemented.

## Runtime, cache, and safety

Preview and export evaluate the same Render Plan scene passes and shader code. Cache identities
include the Composition and Instance Path, scene-pass dependency hash, camera state, relevant local
time range, Module executable hashes, and model/material fingerprints. Changing one camera keyframe
or one instance transform does not recompile unrelated Module Definitions.

Projection values, transforms, hierarchy depth, object count, light count, render-target dimensions,
and per-pass allocations are validated and bounded. Non-finite matrices, parent cycles, invalid clip
planes, and unsupported device capabilities produce diagnostics. They never trigger unbounded graph
expansion or an implicit fallback with different visual semantics.

## Project format

This repository is pre-v1. The canonical spatial transform, Camera source, and explicit nested-space
choice become the only supported model when implemented. No parallel 2D/3D transform store,
compatibility writer, or bidirectional synchronization is introduced. Rendered scene passes, camera
matrices, GPU resources, and picking buffers are derived and are not serialized.

## Consequences

Users can animate 3D position, rotation, hierarchy, and cameras with ordinary Timeline tools while
advanced users build bounded procedural scenes in Node Clips. The staged order keeps the first GPU
slice self-contained, but the accepted coordinate, ownership, camera, and composition contracts avoid
turning that slice into a competing scene model.
