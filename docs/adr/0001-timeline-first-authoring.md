# ADR 0001: Timeline-first authoring model

- Status: Accepted
- Date: 2026-09-03
- Supersedes: the shared Timeline/Graph source-of-truth direction in
  `docs/cloud-runner-handoff-2026-07-20.md`

## Context

The pre-v1 Project stores authored Timeline structure and executable Node
topology in the same graph. Composition, Track, and Clip containers own Node
IDs and structural Merge Nodes. A normal edit therefore changes an execution
graph, and large timelines necessarily create large user-visible graphs.

The product must remain a fast Timeline editor for users who never open a Node
Editor, while retaining reusable procedural logic for users who need it.

## Decision

The authoring model has four non-overlapping responsibilities:

- Timeline owns placement, time, layer order, parent relationships, nested
  Timelines, authored properties, keyframes, masks, mattes, constraints, and
  transitions.
- ModuleDefinition owns reusable Node topology and its published parameters,
  signals, and actions. ModuleInstance owns only a definition reference and
  instance overrides.
- Binding connects external signals or events to published interfaces. A
  Binding cannot address an internal Node UUID.
- RenderPlan is a derived, hierarchical execution structure. It is neither
  persisted nor user editable.

A Composition is a Timeline definition. A placed Composition is a TimelineItem
whose SourceRef contains a CompositionInstance. The path to a nested instance
is the root Timeline ID followed by the TimelineItem IDs crossed on the way
down. Module-internal IDs never participate in this path.

Visual evaluation order is source, item time mapping, pre-transform
attachments, mask/matte, transform/parent/constraint, post-transform
attachments, transition/blend, track post-composite attachments, and Timeline
post-composite attachments. Audio evaluation order is source time mapping,
pre-fader attachments, gain/pan, post-fader attachments, track mix, and
Timeline mix.

The new schema is the first supported Project format and has
`format_version: 1`. Versionless pre-v1 files are rejected. There is no reader,
migrator, compatibility compiler, legacy evaluator, or bidirectional sync for
the old model. The repository tag `pre-b-architecture-20260903` is the recovery
point for the old code.

## Consequences

Timeline complexity no longer determines the number of user-visible Nodes.
The Logic editor projects one selected ModuleDefinition and never expands a
TimelineItem. Existing projects must be recreated after the hard cutover.
Static render fixtures captured from the recovery tag may be used as expected
output, but no old evaluator is linked into the application.
