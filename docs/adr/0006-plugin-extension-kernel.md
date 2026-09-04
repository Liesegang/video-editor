# ADR 0006: Capability-based plugin extension kernel

- Status: Accepted; implementation is staged
- Date: 2026-09-05

## Context

RuViE needs plugins for work that should not be permanently built into the application: media
import and export, generators, analysis such as local speech recognition, speech synthesis, effects,
transitions, audio and MIDI processing, scene rendering, and focused authoring tools. Some of these
extensions are low-bandwidth control operations, while others are real-time or resource-heavy. A
single untyped callback cannot safely or efficiently serve both.

The current repository already has useful foundations:

- `crates/plugin-api` defines a host-independent C ABI, bounded JSON control calls, and negotiated
  typed tables for CPU RGBA8 Effects and Loaders.
- `runtime_native` validates bundle containment, table sizes, descriptors, property defaults, and
  returned buffers. It prepares a complete bundle before atomically registering its components and
  keeps loaded libraries alive until process exit.
- `OperationDescriptor`, `PluginOperationContent`, `OperationRef`, and `EffectContractSnapshot`
  demonstrate the correct missing-plugin property: an authored Project can retain stable operation
  identity, ports, properties, and defaults without executing the plugin.
- `AuthoringSession::transact` is the authoritative validated Project mutation and Undo boundary.
  Timeline plugins must enter through that boundary rather than receiving mutable Project access.

The current system is not yet a general extension kernel:

- Runtime C-ABI components are limited to Effector, Property, Style, Decorator, typed CPU RGBA8
  Effect, and typed CPU RGBA8 Loader. Exporter, Generator, Analyzer, audio, event, MIDI, scene, and UI
  extensions have no runtime adapter.
- `PluginManager` maintains one repository and registration path per Rust trait. Separately,
  `manager/dynamic_loading.rs` loads Rust trait objects that require the exact same toolchain. That
  path overlaps the stable runtime bundle system and is unsuitable as the public extension ABI.
- Component IDs are effectively global, while the bundle descriptor has no stable package ID.
- Inspector plugin lookup currently returns an empty list. There is no declarative Panel contract.
- There is no isolated worker transport. Native plugins are trusted code with all host permissions.
- Some category callbacks, notably Export, are invoked while a registry read lock is alive. No
  plugin callback may run under a global registry lock because re-entry can deadlock the host.
- Timeline has no first-class Transition model or Transition plugin execution contract. The word
  “transition” in structural graph helpers is not an editable video transition feature.

Adding another manager for AI, another UI SDK for panels, or a plugin-owned Timeline API would
duplicate infrastructure and ownership. Conversely, exposing arbitrary Project, renderer, GPU, or
Node internals would make every internal refactor an ABI break and allow plugins to bypass Undo,
validation, dependency tracking, and cache invalidation.

## Decision

RuViE will have one capability-based plugin extension kernel. A package contributes one or more
components at explicit host-owned extension points. A component may implement several compatible
capabilities, but a capability has one descriptor schema, one lifecycle, one data boundary, and one
host adapter.

The phrase “core extension” means implementing a capability at a stable core seam. It does not mean
injecting arbitrary Rust into Project mutation, scheduling, Timeline ownership, Undo, renderer
internals, or application UI internals.

The existing runtime bundle discovery, descriptor validation, and `query_extension` mechanism are
generalized in place. We do not add a second plugin manager. The same normalized package and
component descriptors feed both supported transports:

1. **Trusted in-process ABI** uses versioned C function tables for low-latency typed operations.
2. **Isolated worker RPC** uses a supervised child process for untrusted, blocking, crash-prone, or
   dependency-heavy work such as speech recognition and generative AI.

The legacy same-toolchain Rust trait-object loader is not expanded. Before the public plugin SDK is
declared stable, its remaining users are moved to bundled registration or the C ABI and the loader is
removed. Built-in Rust implementations may continue to implement internal traits for performance,
but those traits are not an external binary contract.

## Ownership invariants

1. Timeline owns placement, time, tracks, layers, parenting, nested compositions, transitions,
   markers, keyframes, and direct edits.
2. `AuthoringSession` and domain editor services are the only Project mutation and Undo authority.
3. A plugin never receives `&mut AuthoringProject`, `AuthoringSession`, an Undo stack, a mutable
   Node graph, or application state.
4. A plugin cannot define a new serialized top-level Project owner. Plugin-specific state lives in a
   bounded opaque envelope attached to a host-defined source, processor, job, or panel contribution.
5. All Project, asset, Timeline, Module, and published-interface references remain typed host-owned
   fields. A plugin may not hide such references inside opaque bytes because hidden references
   cannot be validated, remapped, invalidated, or diagnosed.
6. A plugin receives resolved values, immutable snapshots, and host-minted resource handles. It does
   not receive filesystem paths, GPU pointers, Skia objects, editor widgets, or arbitrary Node UUIDs
   unless a capability explicitly defines that resource.
7. A plugin may request an edit. The core validates and applies that request as one transaction, or
   applies nothing. A plugin cannot commit a partial edit.
8. Render Plan scheduling and cache dependencies remain derived host data. Plugins declare inputs,
   outputs, determinism, statefulness, and invalidation dependencies; they do not schedule
   themselves.
9. Missing, disabled, crashed, or newer plugins never cause authored state to be discarded or
   rewritten. Only the affected invocation produces a diagnostic or no output.
10. VST3, After Effects plug-ins, and other third-party SDKs are hosted behind dedicated adapters.
    Their ABI types, lifecycle, and licensing assumptions are never copied into RuViE's generic ABI.

## Stable identity and descriptor contract

Every installable bundle has a required reverse-domain package ID, for example
`studio.example.caption-tools`. Component IDs are stable within that package. The canonical identity
is the tuple:

```text
PluginComponentKey {
    package_id,
    component_id,
    capability_id,
}
```

Display names, categories, menu paths, and translations are presentation metadata and are not
identity. A package version is semantic-versioned release metadata. Each component additionally
declares a state schema version and every capability protocol version it implements. Runtime
selection is based on the canonical key and a negotiated capability protocol, never a display name
or load order.

The bundle manifest becomes the authority for package identity, transport, executable/library per
platform, declared permissions, and content fingerprint. The callback/worker descriptor must report
the same package ID and version; mismatch rejects the complete bundle. Two bundles cannot claim the
same canonical component key. Registration remains all-or-nothing.

Each authored plugin use stores this host-owned envelope:

```text
PluginStateEnvelope {
    component: PluginComponentKey,
    component_version,
    capability_protocol,
    state_schema,
    encoding,
    payload,
    contract_snapshot,
    bindings,
}
```

- `payload` is bounded opaque bytes preserved exactly. JSON project files encode it canonically;
  large generated media belongs in the managed artifact store and is referenced by fingerprint.
- `contract_snapshot` contains the last validated display label, property/action definitions, typed
  ports, defaults, and schema digest. It contains no executable code.
- `bindings` contains typed host-visible resource and published-interface references. Credentials
  are host vault handles and are never serialized in the Project.

The snapshot lets Inspector, Node Editor, diagnostics, and relinking remain intelligible when the
plugin is absent. Execution requires an installed component whose protocol and contract are
compatible. A component upgrade may declare that it reads a range of state schemas. Otherwise a
versioned, pure migration call produces a new envelope proposal; the host validates it and commits
it through the normal transaction. Failure retains the original bytes. Loading a Project never
silently invokes migration or destructively normalizes an unknown payload.

This missing-plugin preservation is part of the first Project format, not a second compatibility
model. The repository remains pre-v1: no retired Project representation or bidirectional migration
path is introduced.

## Capability families

The following table defines the ownership and boundary of each family. “Control” means bounded
versioned messages. “Typed” means a separately negotiated table or RPC stream with explicit memory,
lifetime, and size rules.

| Capability | Host-owned placement/state | Plugin input and output boundary | Preferred transport |
|---|---|---|---|
| Media Source | Timeline Item interval, transform, automation | local time + parameters -> typed image/audio/scene output | typed in-process or worker |
| Runtime Generator | Source/Module host and published controls | deterministic time/value/resource inputs -> typed media/signal output | typed in-process |
| Background Generator | Job record and apply proposal | immutable artifacts + settings -> staged asset/artifact + edit proposal | isolated worker |
| Analyzer | Asset/range selection and derived artifact reference | immutable media slices -> transcript, beats, features, or metadata artifact | isolated worker |
| Importer | Asset identity, authorization, import settings | granted input handle -> candidates, metadata, decoded resource handles | worker by default |
| Exporter | export range, render schedule, destination grant | host-produced video/audio/event streams -> staged destination | worker or typed in-process |
| Effect | Attachment owner, stage, order, enable/bypass, automation | one typed stage input + evaluated parameters -> same typed media | typed in-process |
| Transition | participants, overlap/range, progress mapping, automation | A/B typed inputs + normalized progress -> typed media | typed in-process |
| Audio Processor | track/bus attachment and automation | fixed audio blocks + transport + parameters -> audio blocks | real-time typed in-process |
| Event/MIDI Processor | clips, routing, bindings, automation | timestamped event batches + transport -> event/signal/action batches | real-time typed in-process |
| Scene Producer | Timeline/Module host and resource bindings | values + scene resources -> validated scene command buffer | typed in-process or worker prepare |
| Scene Pass | Render Plan pass selection and target allocation | host render target/depth/resource handles -> pass completion/fence | trusted render-thread ABI only |
| Inspector contribution | selected host object and envelope | declarative sections/controls/actions; state patch requests | Control |
| Panel contribution | docking instance and envelope | declarative view tree, queries, actions, progress, previews | Control or worker |
| Timeline edit request | selection and explicit user intent | typed request -> core dry-run/validation/transaction result | Control |

### Sources, generators, analysis, import, and export

A plugin Media Source is one `SourceRef` payload on an ordinary Timeline Item. The Item remains
movable, trimmable, splittable, parentable, and automatable without opening Node Editor. A user may
explicitly promote the result to a Node Clip when topology is useful; no source is automatically
expanded into Nodes.

A Runtime Generator evaluates during preview/export and declares whether it is pure, stateful, or
seek-replayable. A Background Generator creates managed artifacts asynchronously. Video generation
services and local models use the background form: they stage output, report progress, and return an
asset/import proposal. They do not insert clips while a job is running.

An Analyzer returns a versioned derived artifact with source fingerprints and time-domain metadata.
For example, a local Whisper-family plugin returns words/segments, confidence, language, speaker
labels when available, and exact source time ranges. A separate core apply command can create or
update Caption Items. Re-analysis can reconcile by stable segment keys; it never silently discards
manual caption overrides.

A speech-synthesis plugin consumes text, voice settings, and optional timing constraints and returns
staged audio plus phoneme/word alignment artifacts. The host then offers one atomic edit proposal to
import the audio and place or replace Items. The plugin does not own caption or Timeline timing.

Importer components only probe and decode granted bytes/resources. They return typed import
candidates and diagnostics; the core owns Asset IDs, deduplication, project-relative policy, and any
nested Timeline construction. Exporters receive the host's evaluated streams and a capability-scoped
staging destination. The core atomically publishes a successful output; Project data cannot choose
an executable or arbitrary output path.

### Effects and transitions

Effects remain ordered Attachments at host-defined evaluation stages. Their descriptor declares
input/output type, color domain, latency, temporal look-behind/look-ahead, in-place safety,
determinism, and parameter schema. The existing CPU RGBA8 table is one concrete protocol, not the
universal Effect representation. Project-linear CPU, GPU texture, audio, and scene protocols require
separate typed contracts and explicit conversions chosen by the Render Plan compiler.

Transition is a first-class Timeline-owned relation, not an Effect with hidden neighbor lookup. The
core model identifies both participants, its exact interval, alignment, progress mapping, and audio
and video policies. A Transition component receives only the evaluated A/B inputs, normalized
progress, local time, and parameters. It cannot move or trim either Item. Missing transition code
preserves the relation and snapshot and produces a localized diagnostic.

### Audio, event, and MIDI processing

Audio callbacks operate on fixed, preallocated channel buffers with sample rate, block position,
latency, transport discontinuity, and offline/realtime mode. A real-time callback must not allocate,
lock, perform file/network I/O, spawn work, invoke UI, or log synchronously. Instance creation,
state loading, buffer sizing, and teardown happen on control threads. The host delay-compensation and
routing graph owns scheduling.

Events are timestamped records; continuous signals are sampled values. MIDI input and output use a
versioned event representation with sample offsets within the audio block and an explicit mapping to
Published Signal/Event/Action interfaces. A MIDI NoteOn is not emulated by writing an arbitrary
property. Offline export consumes the same ordered event stream deterministically.

VST3 hosting is a dedicated component adapter layered behind Audio Processor and Event/MIDI
Processor capabilities. It owns VST3 discovery, class IDs, buses, parameter queues, state chunks,
latency changes, editor windows, and SDK-specific thread rules. None of those structs appear in
`ruvie-plugin-api`'s generic descriptor or wire types.

### Scene producers and passes

Ordinary scene plugins produce a bounded, validated, backend-neutral scene command buffer and
references to host-managed meshes, textures, materials, and buffers. They never receive a raw Skia
object or unchecked OpenGL pointer. The Scene Runtime validates counts, formats, access modes,
resource lifetimes, and deterministic cache dependencies before execution.

A low-level Scene Pass is a separate trusted capability for implementations that genuinely need GPU
commands. It runs only on the render thread, inside the Scene Runtime's existing state-save,
synchronization, and budget boundary, against host-created targets. Backend/version negotiation is
mandatory. Worker plugins may prepare resources or command buffers but do not directly own the
interactive GPU context.

FBX/model decoding remains a producer of the shared `MeshScene` resource described by ADR 0005;
plugins do not create format-specific renderers. Particle and Plexus-like renderers remain scene
producers/passes in the single ADR 0003 runtime rather than parallel compositors.

### Declarative Inspector and panels

Plugins contribute data, not `egui` code. The host renders a bounded declarative schema through the
existing shared property widgets, modal primitives, docking system, icons, theme, accessibility, and
QA metadata. The initial schema supports sections, labels, validated property editors, tables,
progress, diagnostics, media thumbnails, and actions. Arbitrary native child windows or HTML are not
part of the first contract.

An Inspector contribution is scoped to one selected host object and its plugin envelope. Editing a
control emits a typed state patch or domain command request; it cannot mutate the object directly. A
Panel contribution has explicit instance state, subscriptions, and actions and is docked like any
other panel. Workspace presets may include it, but a plugin cannot rearrange the user's layout.

Declarative actions declare confirmation level, concurrency, cancellation, and required permission.
The host decides presentation and always uses shared dialogs. Descriptor-provided strings are
untrusted display data and are size-bounded.

## Timeline edit requests

Plugins can request only versioned commands from a host-owned allowlist. The first command set is
deliberately outcome-oriented rather than a mirror of Project structs:

```text
ImportStagedAsset
CreateItemsFromAssets
CreateOrUpdateCaptionSegments
CreateMarkers
CreateAutomationSamples
CreateNestedTimelineFromLayers
ReplacePluginState
```

Every request includes the Project revision it observed, typed target IDs, an explicit scope, stable
result keys, and expected preconditions. The core performs a dry run, resolves current IDs, checks
permissions and capability-specific limits, shows a confirmation when destructive or broad, then
uses the relevant domain editor service inside one `AuthoringSession::transact`. The response maps
plugin result keys to host-created IDs and reports a `ChangeSet`.

Stale preconditions, invalid timing, ownership violations, unavailable resources, excessive item
counts, or unsupported commands reject the whole request. Undo reverses the complete accepted
request in one step. Redo does not rerun an AI model or external side effect; it reapplies the
recorded core edit. Long work therefore finishes before edit application, and its immutable staged
artifacts are retained according to a bounded Undo policy.

This command boundary is also used by built-in panel actions where practical. It must not become a
second generic “patch Project JSON” API.

## Transport and execution model

### Trusted in-process ABI

The in-process ABI remains a small versioned C table with an opaque context, bounded control calls,
and `query_extension` for capability-specific typed tables. Tables begin with ABI version and size;
all buffers and handles have explicit allocator, owner, and release rules. No Rust trait object,
`serde_json::Value` frame, Project type, or renderer object crosses a hot path.

In-process code has the user's full process permissions and can corrupt memory or crash the host.
It is enabled only for explicitly trusted packages. Libraries are never hot-unloaded. A replacement
is activated after restart so no callback, cache, or function pointer can outlive its library.

### Isolated worker RPC

The first worker protocol is a supervised child process using length-prefixed, size-bounded UTF-8
JSON control envelopes over private pipes. Standard error is captured as bounded diagnostics. Large
or hot data never appears in JSON: the host supplies read-only artifact tokens, bounded shared-memory
regions, or staged-output tokens defined by a capability protocol.

Handshake negotiates kernel protocol, package identity, component capabilities, limits, and granted
permissions before any job starts. Every call has a request ID, deadline, cancellation token, and
maximum response size. The host limits concurrent workers, resident memory where the OS permits,
CPU/GPU use, staged bytes, and log volume. Timeout, malformed output, pipe closure, or process death
fails the current operation without committing its edit proposal. Restart policy is bounded and is
never used for a real-time audio callback.

Process separation is crash isolation, not automatically a security sandbox. Production packaging
must add platform sandboxing and signing. Until then, the UI describes the effective trust level
honestly.

### One semantic dispatcher

Descriptor normalization, identity resolution, permission checks, request validation, result
validation, diagnostics, lifecycle, and capability adapters are transport-independent and have one
implementation. The transport layer only sends bounded calls and resources. In-process and worker
endpoints must pass the same conformance suite; they do not each reimplement Effect, Analyzer, or
edit-command semantics.

## Lifecycle and threading

The package lifecycle is:

```text
discover manifest
-> validate identity/fingerprint/permissions without executing plugin code
-> start worker or load trusted library
-> handshake and snapshot descriptor once
-> validate and atomically register every component
-> instantiate per job/invocation/session
-> prepare resources
-> process
-> flush/checkpoint when defined
-> release instance
```

The manager clones an immutable component endpoint and drops every registry lock before invoking a
plugin callback. Registry mutations publish a new generation and invalidate only dependent compiled
plans/caches. Plugin destruction and worker shutdown also occur outside registry locks.

| Execution class | Allowed work | Required behavior |
|---|---|---|
| Control | descriptor, state, validation, UI actions | bounded; may allocate; no UI-thread blocking |
| Background | import, analyze, generate, synthesize, prepare | cancellable; progress; deadlines and budgets |
| Render worker | CPU frame/effect/source evaluation | thread model declared; deterministic inputs |
| Realtime audio | audio/event/MIDI blocks | no allocation, locks, I/O, waits, panics, or synchronous logs |
| GPU render | scene prepare/execute | render-thread only; host context and resource scopes |

Descriptors declare whether instances are immutable, thread-confined, or reentrant. The host never
assumes `Send + Sync` merely because a C table exists. Preview and export use separate mutable
instances where history could affect output while sharing immutable compiled resources.

## Background jobs and artifacts

The core owns a persistent job ledger separate from Project source of truth. A job records component
identity, validated settings, granted immutable input fingerprints, state, progress, diagnostics,
cancellation, and staged result artifacts. Runtime process handles and model memory are not
serialized in the Project.

Artifacts have content fingerprints, media type, byte size, producer identity/version, source
fingerprints, settings digest, and optional stable element keys. A result can be previewed before it
is applied. Applying it stores only core Asset/Timeline state plus provenance needed for refresh and
override reconciliation. Deleting or changing source data marks unmatched manual overrides as
orphaned/conflicting; plugins cannot silently erase them.

Remote generation plugins use the same job contract as local models. Network access, endpoint
origins, cost-bearing actions, credentials, and data upload are explicit grants. A plugin called
“local” must operate without a network grant.

## Permissions and security

Capabilities are necessary but not sufficient permissions. Packages separately request:

- read access to user-selected assets through read-only handles;
- staged output bytes and final export publication;
- network access to named origins;
- microphone, audio device, MIDI device, or camera access;
- GPU compute or trusted low-level scene pass access;
- child-process/model-runtime use; and
- credential handles for a named service.

The default worker grant is none. Project files cannot grant permissions, select executable paths,
or smuggle filesystem/network locators through opaque payloads. Grants are installation/user state,
not portable Project authority. All counts, dimensions, durations, timestamps, channel layouts,
event sizes, nesting depth, strings, payloads, shared memory, and output bytes are bounded and
validated before allocation or dispatch.

In-process trust is an explicit stronger grant because an ABI validator cannot sandbox arbitrary
native code. Package fingerprint changes revoke prior binary trust until the user or a verified
signature policy accepts the replacement.

## Failure semantics

- Missing/disabled component: preserve envelope and snapshot; show a repair action; emit localized
  no-output/silence only where the capability contract defines it.
- Background failure/cancellation: commit no Project edit; retain bounded diagnostics and clean
  unreferenced staging data.
- Import failure: create no half-valid Asset.
- Export failure: never publish or overwrite the destination with a partial result; use staging and
  atomic publication where the platform supports it.
- Effect/Transition failure: return a structured diagnostic. Bypass is allowed only when the
  authored failure policy requests it; failure is not silently converted to success.
- Realtime processor failure: deactivate at a safe block boundary and use the authored safe policy
  without blocking the callback.
- Worker crash or protocol violation: invalidate its runtime instances, preserve authored state, and
  require bounded restart/retry from a deterministic boundary.

## Delivery stages

### Stage 0: consolidate the kernel

- Add stable package identity and capability declarations to the bundle manifest/descriptor.
- Introduce the canonical component registry, `PluginStateEnvelope`, contract snapshot, permission
  model, and one transport-independent dispatcher.
- Move existing runtime-native categories through that registry without changing their editing
  semantics.
- Ensure all callbacks run outside registry locks.
- Retire the same-toolchain external Rust trait-object loading path rather than maintaining two
  public plugin systems.

### Stage 1: worker jobs and safe authoring requests

- Implement worker handshake, supervision, cancellation, progress, artifact staging, and limits.
- Implement declarative Inspector/Panel schema using shared application widgets and dialogs.
- Implement the first typed Timeline edit requests with dry-run, confirmation, one transaction, and
  one Undo step.
- Prove a post-build local transcription plugin end to end: media grant -> transcript artifact ->
  caption proposal -> atomic apply, with no dependency on `app` or `library`.

### Stage 2: media and authoring capabilities

- Externalize runtime Importer and Exporter through typed protocols.
- Add Runtime Source, Background Generator, Analyzer, speech synthesis, and template/layer import
  capabilities.
- Add first-class Timeline Transition and its typed image/audio adapter.
- Preserve stable provenance and manual overrides across regenerated results.

### Stage 3: real-time audio, MIDI, and dedicated SDK hosts

- Add real-time Audio and Event/MIDI tables, latency reporting, deterministic offline mode, and
  realtime-safety instrumentation.
- Implement VST3 as a dedicated isolated/scanned host adapter behind those capabilities.
- Keep After Effects effect compatibility in its own dedicated adapter/process and expose only the
  subset actually implemented; do not claim arbitrary AEGP/AEIO/extension compatibility.

### Stage 4: scene extension

- Add the validated scene command/resource protocol to the existing Scene Runtime.
- Add trusted backend-specific Scene Pass negotiation only after resource lifetime, GPU state,
  device-loss, and preview/export parity tests exist.
- Route model, particle, and Plexus-like capabilities through the same scene compiler/runtime.

## Acceptance tests

The kernel is not complete until these tests exist:

1. A standalone plugin built after the host depends on `plugin-api` only, is discovered, and executes
   through the unchanged host binary.
2. In-process and worker fixtures with the same descriptor pass one identity, validation, state,
   permission, command, and failure conformance suite.
3. Duplicate package/component keys, descriptor mismatch, unsupported protocols, malformed defaults,
   and one invalid component reject the whole bundle without partial registration.
4. A Project containing an absent Source, Effect, Transition, and Panel state loads and saves with
   byte-identical opaque payloads, typed bindings, and contract snapshots.
5. A failed or incompatible state migration retains the original envelope. A successful migration
   is explicit, validated, and Undoable.
6. A plugin cannot address undeclared files, network origins, Project IDs, internal Node UUIDs, or
   output paths. A Project document cannot elevate installation permissions.
7. A stale or invalid Timeline edit request changes nothing. An accepted multi-item request is one
   Undo/Redo operation and does not rerun external work on Redo.
8. Cancelling or crashing a background transcription/generation/synthesis worker changes no Project
   state and leaves no published partial file.
9. The transcription fixture preserves exact source ranges and stable segment keys when captions are
   applied and refreshed with a manual override.
10. Effect and Transition fixtures compare preview/export frames and timing. A Transition cannot
    inspect or move neighboring Items outside its typed A/B inputs.
11. Audio/MIDI fixtures verify sample-offset ordering, latency changes, transport discontinuity,
    deterministic offline output, and zero callback-thread allocation/locking/I/O.
12. Scene fixtures reject invalid handles and excessive commands, restore graphics state, survive
    device loss with diagnostics, and match preview/export output.
13. Declarative Inspector/Panel fixtures use the shared widgets, dialog layout, docking, theme,
    accessibility, and loopback QA metadata; plugin code does not render a parallel UI surface.
14. Reentrant callbacks prove no global registry lock is held. Concurrent scan/register/invoke and
    worker termination cannot deadlock or expose partially registered components.
15. Fuzz/property tests cover manifests, descriptors, envelopes, RPC framing, shared-memory bounds,
    command validation, and malformed plugin return values.

## Consequences

RuViE can add deep domain capabilities without turning plugins into alternate owners of the editor.
Beginners continue to use Timeline, Inspector, Effects, transitions, captions, and templates without
seeing implementation Nodes. Advanced plugins can participate in rendering, audio, MIDI, 3D, and
automation through explicit high-performance contracts.

The cost is that every new capability requires an honest host adapter, lifecycle, validation,
failure policy, and test suite. A plugin cannot “just call core internals.” This restriction is what
keeps Projects editable, Undoable, inspectable when plugins are missing, and refactorable without
freezing the entire application as its ABI.
