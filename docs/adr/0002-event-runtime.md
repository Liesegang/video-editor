# ADR 0002: Signals, events, and reactive instances

- Status: Accepted for the public contract; runtime implementation is deferred
- Date: 2026-09-04

## Context

Continuous modulation and discrete commands have different time semantics. Treating a MIDI note,
beat, or cue as a direct write to `local_time` makes playback state implicit, prevents deterministic
queuing, and cannot represent overlapping responses. Nested Timelines also require a concrete
instance path: a Definition ID alone does not identify which placement receives the control.

## Decision

RuViE keeps two public connection kinds.

- A `SignalBinding` samples a continuous source, applies mapping and smoothing, and contributes to
  the effective value of a published parameter.
- An `EventBinding` delivers a discrete event to a published action using an explicit trigger
  policy.

Both binding kinds address only published interfaces. A target contains an `InstancePath`, a Module
Instance ID, and a Published Interface ID. No external binding may address an internal Node ID or
perform an arbitrary property write.

Event actions use these policies:

- `Restart`: restart the addressed playback instance at its local origin.
- `IgnoreWhilePlaying`: discard the event while that instance is active.
- `Queue`: retain events in deterministic timestamp and insertion order, then start each response
  after the previous response ends.
- `Overlap`: create a transient reactive playback instance for each accepted event.

An overlapping response is not implemented by mutating one persistent instance's clock. The
runtime creates an ephemeral child identity beneath the addressed `InstancePath`; its key includes
the binding ID and a monotonic event sequence. Ephemeral instances are runtime state and are never
serialized as Timeline Items. Their lifetime is bounded by the action response or an explicit stop.

The persisted project owns binding declarations and authored defaults. The Render Plan owns routing,
event scheduling, and dependency ordering. Runtime state owns envelopes, queues, active reactive
instances, and playback cursors. Evaluation at a project time is deterministic for the same authored
project and ordered event log.

## Effective-value provenance

Signal evaluation retains contributions separately: authored base, keyframe, each named signal
binding, and manual override. The Inspector may display their composition and must not silently
disconnect a binding after direct manipulation. Properties whose operators cannot support a
non-destructive override must ask the user whether to replace the binding.

## Consequences

Audio and MIDI can drive either smooth motion or discrete playback without conflating the two.
Repeated nested Composition placements remain independently addressable. Implementing the event
runtime requires bounded queues, cancellation rules, and resource limits for `Overlap`; those are a
separate vertical slice and do not block Timeline-first editing or Node Clips.
