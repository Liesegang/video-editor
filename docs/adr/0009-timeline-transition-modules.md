# ADR 0009: Timeline-owned transitions with bounded Node Modules

- Status: Accepted
- Date: 2026-09-05

## Context

A transition has two separate concerns. Its participants, edit point, duration, alignment, and
placement are Timeline editing decisions. The image or audio processing that combines the two
sources may be built in, reusable, or procedural. Representing both concerns as one graph would
expand ordinary clips into Nodes and make Timeline structure another graph-owned source of truth.

RuViE also permits the same nested Timeline definition to be placed more than once. Controls for a
transition inside that definition must be addressable per concrete placement without cloning the
Timeline or its processing definition.

## Decision

`Transition` is a first-class Timeline-owned record. It owns stable references to the A and B
Timeline Items, its edit point, duration, alignment, media type, and processor invocation. Neither
participant is represented as a user-facing Node.

A processor is either a typed built-in operation or a bounded Transition Module. A Transition
Module Definition owns only processing topology and exposes a stable host contract:

- published A media input;
- published B media input;
- published normalized Progress parameter;
- one typed Output.

The Timeline host supplies A, B, and Progress. Their published input targets are protected from
deletion, rewiring, inline value editing, keyframes, and expressions. Their output ports remain
available for internal graph wiring. The Output terminal's media input remains internally
connectable. Module-internal Node UUIDs are private and are never persisted by Timeline bindings.

Image Transition Modules may publish additional Image inputs for mattes or auxiliary clips. Audio
Transition Modules currently accept only the host-owned A and B streams because the audio mixer has
no runtime for additional media inputs. Unsupported media types and processing Nodes are rejected
while authoring, rather than saved and rejected only during rendering.

`RenderPlan` compiles each Module Definition once and stores lightweight Transition invocations.
The plan remains hierarchical; it does not flatten source clips or duplicate processing topology
for each transition.

For a transition inside a nested Timeline, definition-scope controls live on the Timeline-owned
processor invocation. A concrete Composition placement may persist a sparse override using an
owner-relative Composition path, `TransitionId`, `ModuleInstanceId`, and published interface IDs.
Compilation derives the full `InstancePath`. Runtime state and invalidation use that concrete path,
so two placements may differ while sharing one Timeline and one Module Definition.

The production Node Editor is the only topology editor. Opening customization promotes a supported
built-in transition to a private starter Module containing finite A, B, Progress, Mix, and Output
Nodes. Timeline Items are not expanded. Published parameters are edited through the normal
Inspector, Dope Sheet, and Curve Editor automation path in transition-local time.

## Invariants

1. Moving, trimming, or changing the duration of a Transition cannot change Module topology.
2. Editing Transition Module topology cannot change participants, edit point, duration, or layer
   order.
3. A Transition Module cannot reference a participant or external clip by an internal Node UUID.
4. Host-owned A, B, and Progress targets cannot be deleted, duplicated, authored, or externally
   rebound.
5. A concrete nested override changes only its exact `InstancePath`; sibling placements inherit the
   definition-scope controls.
6. One compiled Module Definition may serve any number of Transition invocations.
7. The derived RenderPlan is neither serialized nor user-editable.
8. The pre-v1 project format has no compatibility reader, writer, or bidirectional legacy model.

## Consequences

Simple transitions remain ordinary Timeline operations. Advanced users can customize only the
processing island that needs Nodes, including auxiliary published Image inputs and animated
published parameters. Nested instances remain independently controllable without multiplying Node
graphs or creating a second owner for Timeline state.

Reusable Transition template management, assignment of required auxiliary inputs, and additional
audio/plugin runtimes may extend this contract. They must preserve the same typed published
boundary and may not expose Timeline structure as Nodes.
