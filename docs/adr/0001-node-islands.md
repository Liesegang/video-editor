# ADR 0001: Timeline authoring with bounded Node Modules

- Status: Accepted
- Date: 2026-09-04

## Context

The pre-release project model makes Composition, Track, and Clip node containers. A normal edit
therefore has both timeline structure and graph structure, and the user-facing graph grows with the
number of clips. This duplicates ownership and makes a large project graph impractical to edit.

RuViE must remain a direct-manipulation timeline editor for ordinary work while allowing procedural
video, motion graphics, audio, MIDI, and project-wide control to use nodes where that representation
is useful.

## Decision

Timeline and node graphs model different things and never mirror each other.

- A Timeline owns placement, intervals, time mapping, layer order, hierarchy, nested timelines,
  masks, mattes, transitions, authored properties, and automation.
- A Module Definition owns a bounded processing graph and its published interface.
- A Module Instance owns a Definition reference and instance-local constant parameter values.
- A Module Invocation is owned by one host and supplies host-local output selection, media bindings,
  and automation.
- A Render Plan is derived, hierarchical runtime data. It is neither serialized nor user-editable.

The first Module host is `TimelineSource`: a Timeline Item whose source is a Module Invocation. The
UI calls this a **Node Clip**. A Node Clip moves, trims, splits, duplicates, and nests like any other
Timeline Item. Its graph does not own placement or duration; the host supplies project and local time.

The same Invocation contract may later be hosted as an Item processor, Track processor, Bus
processor, Master processor, or Project Control module. Host compatibility is inferred from typed
published ports rather than an exclusive Generator/Effect/Analyzer role.

External references may address only published parameters, signals, actions, event ports, and media
ports. An external binding never addresses an internal Node UUID. Dragging a Timeline source into a
Module editor creates or binds a published boundary input; it does not expand that source's timeline
or graph.

Nested Composition is represented as a Timeline source referring to another Timeline. It is not a
Node type. Ordinary built-in effects are ordered Timeline-owned processor attachments and do not
implicitly create Module Definitions or Nodes.

## Invariants

1. Creating, moving, trimming, splitting, or deleting an ordinary Timeline Item does not create a
   user-facing Node.
2. A Module graph contains processing, generation, analysis, or procedural nodes only. Timeline,
   Track, Clip, and Composition Instance nodes are invalid.
3. Every Module Instance is owned by exactly one Invocation. Definitions may be shared; runtime
   state is keyed by Instance Path and Instance ID.
4. Timeline automation may target authored Item properties, built-in processor parameters, or
   published Module parameters. It may not target arbitrary internal Node properties.
5. Moving or trimming a Node Clip cannot change its Module Definition. Editing a Module Definition
   cannot change Timeline placement.
6. A compiled Module Definition is shared by all matching invocations. Presentation-only graph
   changes are excluded from the executable cache key.
7. Zero-delay media, signal, or event cycles are rejected with the dependency chain. A feedback
   loop requires an explicit delay/state boundary.
8. `RenderPlan` complexity is proportional to authored Timeline Items plus Module Invocations; a
   Module graph is never expanded once per instance.

## Editing behavior

- A newly created empty Node Clip starts with a private Definition.
- Placing a saved Module creates a new Instance with local values while sharing its Definition.
- Editing a placed shared Module uses copy-on-write by default. Editing the shared template is an
  explicit operation that reports how many instances will change.
- Splitting or duplicating a Node Clip creates a new Instance and stable automation identities while
  continuing to share the Definition.
- Removing an interface or referenced source reports affected invocations and requires remapping or
  explicit cascading; references are never silently discarded.

## Inline Node Clip animation authoring

The Node Clip input clock exposes an unconnected constant input as a Published Parameter and
creates its first Invocation-owned automation key in one transaction. The existing instance
copy-on-write operation isolates shared Definitions before publication. Undo removes the public
input and first key together; Redo restores their stable identities. No keyframes are stored in
Node properties, and connected, protected, expression-driven, or runtime constant-only inputs
cannot be silently converted to Timeline animation.

After publication, the Node body and Inspector use one parameter editing controller. It resolves
the Invocation's automation and Instance defaults, maintains an Instance-qualified draft, projects
held edits through the existing transient Preview path, and commits once on release. Timeline and
Curve Editor address the same Published Parameter and Keyframe IDs. The host supplies exact
`MediaTime`; floating-point seconds are only an evaluation/display projection. Revision and host
checks prevent a retained document from authoring against a different Timeline or nested path.

This inline authoring slice targets Node Clips. Attachment and Transition inline input editing
remain separate follow-up work using their existing Invocation owners, not parallel graphs.
Keys inside a nested Timeline are owned by that Timeline definition and affect all its concrete
Composition placements; these controls do not create per-placement sparse overrides.

## Project format and transition

This repository is pre-v1. The new versioned document is the first supported project format. No
reader, writer, migration, backup, compatibility evaluator, or bidirectional synchronization is
provided for the retired versionless graph-owned format. Visual and timing parity is checked against
golden output captured from the recovery tag, without retaining the old evaluator in production.

## Consequences

Beginners can finish work without opening the Node Editor. Advanced users edit small, reusable Node
islands and connect them through stable published boundaries. Project-wide processing remains
possible through explicit Track, Bus, Master, and Project Control hosts without turning every clip
into a node.
