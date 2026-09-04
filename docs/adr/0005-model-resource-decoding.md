# ADR 0005: Model resource decoding

- Status: Accepted; first owned FBX decode/resource slice implemented, renderer integration pending
- Date: 2026-09-04

## Context

The importer originally classified OBJ, glTF, and GLB paths as `Model3D`, while the Render Plan
rejected `Model3D` assets and no mesh decoder existed. Image/video stream loaders cannot
represent a scene hierarchy, mesh buffers, materials, or external texture dependencies, so extending
the pixel loader contract would create the wrong ownership boundary.

FBX parsers commonly expose borrowed parser memory and paths supplied by the source file. Keeping
those pointers alive or resolving arbitrary external paths would make cache lifetime and asset import
unsafe.

## Decision

RuViE adds one model-resource decoding service beside, not inside, pixel and audio decoding. All
model formats produce the same owned `MeshScene` representation used by Inspector preview, Timeline
rendering, Node Modules, thumbnails, and export. Those consumers never invoke a format parser or keep
format-specific scene data.

The initial FBX decoder uses the pinned `ufbx` 0.11.3 implementation. Decoder-owned arrays, strings,
and transforms are validated and immediately copied into owned RuViE buffers; all ufbx memory is then
released. No raw or borrowed decoder pointer crosses the decode call. Future OBJ and glTF support
must enter through the same service and `MeshScene`, not through parallel renderer-specific loaders.

The first supported geometry subset is:

- static triangle meshes and node hierarchy;
- local transforms;
- vertex positions, normals, and UV set 0; and
- per-node material-slot bindings for instanced geometry; and
- a base material with resolved base-color texture or value.

Skinning, skeletal animation, morph targets, additional UV sets, advanced material graphs, lights,
and embedded cameras are outside the first slice. The decoder records explicit diagnostics for every
encountered unsupported feature; it does not silently claim full FBX fidelity. A resource with no
renderable supported geometry fails with an actionable error.

Axis, handedness, unit scale, triangulation, and normal generation are normalized exactly once during
decode under versioned settings. Original metadata needed for diagnostics is retained, but renderers
do not repeat format-specific coordinate conversions.

## Resource identity and caching

The Project stores the source Asset reference, imported content fingerprint, and authored import
settings. `MeshScene`, vertex/index buffers, uploaded textures, and parser data are derived and are
never project source of truth.

One shared model cache serves every editor and render path. A decoded-scene key includes source
content fingerprint, decoder identity and pinned version, normalization settings, supported-feature
version, and fingerprints of resolved external resources. GPU upload keys additionally include the
scene content key, vertex layout, material variant, and Scene Runtime capability profile. Preview and
export therefore consume identical decoded values while retaining independent execution sessions.
Concurrent requests with the same semantic key and decode limits join one keyed in-flight decode;
successes and structured failures wake every waiter. Decode limits are part of the flight identity,
but not the derived scene identity, because they change whether a request may succeed rather than its
decoded meaning.

Changing an external texture invalidates the dependent material and GPU resources without decoding
unrelated models. Cache entries have bounded byte and object budgets and can be rebuilt from source.

## External resources and security

Embedded buffers and textures are accepted only after size and count validation. Relative external
texture paths resolve from the canonical model directory and must remain within an explicitly allowed
asset root after canonicalization, including symlink resolution. Absolute paths, traversal outside an
allowed root, network locations, and implicit environment search paths are rejected unless the user
has explicitly imported and authorized the resource.

Decode work runs off the UI thread with hard limits for source bytes, hierarchy depth, node and mesh
counts, faces, vertices, indices, material and texture counts, decoded image bytes, parser arenas,
cumulative Rust-side working storage, and final resident scene bytes. Parser, working, and resident
budgets are explicit independent limits: callers can therefore also bound their combined peak at the
scheduling boundary rather than relying on a misleading single allocation number.
Integer conversions and index ranges are checked before allocation or upload. Malformed files,
unsupported compression, timeout or budget exhaustion, and missing dependencies return structured
diagnostics and cannot leave partially published cache entries.

## Render and export parity

The Inspector, Timeline, Node Editor, preview renderer, and exporter resolve a model through the same
model service and shared resource cache. The ADR 0003 Scene Runtime is the only owner of GPU uploads
and drawing. Neither UI preview nor export implements a private FBX path. Golden fixtures cover static
hierarchy, coordinate normalization, materials, malformed input, external-path rejection, and
preview/export image parity.

## Project format

The repository is pre-v1. FBX support is introduced only through the new Asset and `MeshScene`
contract. No compatibility reader, duplicate persisted mesh representation, or migration from the
current extension-only classification is added. Unsupported-feature diagnostics remain derived import
metadata; they do not make parser internals part of the Project format.

## Implementation status

The first implemented slice provides the format-neutral, fully owned `MeshScene` contract and one
`ModelResourceService` backed by the application's shared rebuildable cache. `ufbx` is pinned to
0.11.3. The service copies normalized static triangle geometry, hierarchy transforms, normals, UV0,
base colors, and embedded base-color texture bytes before releasing parser memory. FBX source,
parser-memory, hierarchy, object-count, vertex/index, texture, and aggregate owned-memory limits are
enforced. Rust-side maps, triangulation scratch, and generated-normal accumulation use fallible,
budgeted allocation; allocator failure is reported separately from configured-budget exhaustion.
Runtime file loads verify the Asset's import-time SHA-256. Shared-geometry nodes
retain their own material-slot tables, and identical concurrent requests are single-flight decoded.

Automatic external file loading is disabled and the parser callback rejects every external open.
External textures and encountered animation, skinning, morph, geometry-cache, camera, light,
additional-UV, texture transform/wrap/layer/shader, and richer-material features are reported as
explicit derived diagnostics. `.fbx`
classification now uses the same case-insensitive Asset fallback as every other import path.

Scene Runtime/GPU upload and drawing, authorized external-texture resolution, decoded texture pixels,
OBJ/glTF producers, Inspector model preview, and preview/export golden parity remain pending. Thus this
slice makes FBX a real decoded resource but does not yet claim that `Model3D` Timeline items render.
Background scheduling, cancellation/time budgets, and binary-FBX golden coverage also remain pending;
the current service call is synchronous and must not be invoked directly from the UI thread.

## Consequences

FBX becomes one safe producer of a format-neutral scene resource rather than a special renderer path.
The deliberately small first subset can be implemented and tested honestly, while skinning,
animation, morphs, and richer materials remain visible follow-up work. This decision does not make
the current `Model3D` classification renderable by itself.
