# ADR 0002: Event Binding runtime

- Status: Accepted
- Date: 2026-09-03

## Context

An Event Binding represents a discrete action. Treating an event as a write to
`local_time` would mix event state with authored Timeline state and cannot
represent queued or overlapping playback.

## Decision

Event triggers create non-persisted reactive invocations addressed through a
Binding scope, a PublishedAction ID, and a runtime-only occurrence ID. They do
not mutate Timeline properties or ModuleDefinition topology.

The policies have these meanings:

- Restart removes active or queued occurrences for the Binding and starts one
  occurrence at the trigger time.
- IgnoreWhilePlaying drops the trigger while an occurrence is active or
  queued; otherwise it starts one occurrence immediately.
- Queue schedules a new occurrence after the latest active or queued
  occurrence for the Binding.
- Overlap starts a new occurrence immediately without changing existing
  occurrences. Each occurrence is a separate reactive instance.

The runtime is derived state. It is reset when playback stops or a Project is
closed and is never serialized into the Project document or RenderPlan cache.
The event source adapter (MIDI, marker, or a published signal edge) supplies a
monotonic trigger timestamp. The RenderPlan runtime consumes the returned
invocation and resolves its public action at the Binding scope; it must not
address an internal Node UUID.

## Consequences

Repeated triggers are deterministic for a given ordered trigger stream.
Overlap does not seek a shared instance. Queue scheduling remains explicit,
and authored local time remains unchanged by reactive playback.
