# node-editor-ui

`node-editor-ui` is a domain-neutral immediate-mode Node Editor for egui. It
can render and interact with a host graph without depending on RuViE, video
types, UUIDs, plugins, or a persisted graph model.

## Ownership contract

The host is the only source of truth. Every frame it lends descriptors and
applies returned intents to its own model and history:

```text
host graph / selection / history
            |
            | borrowed GraphFrame
            v
      node-editor-ui
            |
            | EditorOutput intents
            v
         host adapter
```

`GraphFrame` borrows flat descriptor slices for Nodes, concrete Ports,
authored Wires, and nested Groups. IDs and `TypeKey` values are opaque host
types. Geometry is frame-local and uses one graph-to-screen transform. Its
single back-to-front `selection_order` defines cross-kind Node/Group hit and
marquee-primary policy; lookup-map iteration is never an interaction policy.

`InteractionState` retains only the active pointer gesture. It never retains
Nodes, positions, connections, authoritative selection, undo data, or a render
cache. Dropping it loses no project data.

## Reusable surface

`Editor::show` owns generic group/node chrome, ports, wires, selection
presentation, and interaction orchestration. A host implements
`NodeBodyRenderer<NodeId>` for domain-specific body controls. A Node's explicit
`header_rect` is its movement handle, while body controls combine their real
egui responses into `NodeBodyResponse`; a slider or drag value therefore owns
its drag instead of moving the Node. The same frame path emits:

- `Select` and `DeselectWire`
- `Move` and `ResizeGroup`
- `Connect` and `Disconnect`
- `Delete` and `Reparent`

`tests/fake_host.rs` renders nested groups and host bodies in headless egui and
drives real pointer/key frames for every intent without importing a RuViE or
video type.

## RuViE adapter status

RuViE's production Node Editor now projects its authoritative `Project`,
rendered geometry, actual Snarl draw order, and `SelectionState` through
`app/src/ui/panels/node_editor/surface.rs`. Blank click, marquee, Node/Group
selection, and wire deselection use `Editor::interact` and the same
`InteractionState` as the standalone renderer. Selection is no longer
implemented as a parallel app-only gesture state machine. Overview scale keeps
large Node/Group click targets and blank deselection while precise marquee and
wire selection remain gated.

This is a usable reusable-core vertical slice, **not completion of the RuViE
renderer migration**. The following production behavior still belongs to the
existing Snarl adapter and must move in coherent slices:

1. Node/Group chrome and Project-specific body/port controls.
2. Node movement plus multi-selection position application.
3. Group edge resize and nested containment constraints.
4. Connect/reconnect/disconnect, wire knife, and connection context menus.
5. Reparent/drop scoring and Merge physical-layer reorder gestures.
6. Snarl viewport/layout ownership and RuViE HTTP QA publication.

The generic intents already cover those operations so later slices can replace
adapter ownership without changing the public graph contract or inventing a
second model.

Run `scripts/check-node-editor-ui-boundary.sh` to enforce the dependency and
production-adapter direction. The repository quality gate runs it as well.
