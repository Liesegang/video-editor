# node-editor-ui

`node-editor-ui` is the domain-neutral egui layer of RuViE's Node Editor. It
is a workspace package now so it can later move to its own repository without
reversing the dependency direction.

## Ownership boundary

The host remains authoritative for graph entities, positions, connections,
containment, selection, history, and persistence. This crate may own only
transient UI policy and geometry. It must not depend on `app` or the video
`library`, and it must not introduce a second persisted graph model.

```text
authoritative Project
        | per-frame projection
        v
app Node Editor adapter -----> node-editor-ui
        ^                           |
        +---- domain-neutral results
```

The app adapter currently lives under
`app/src/ui/panels/node_editor/`. It resolves Project-specific ports, clip and
track containment, plugin properties, undo history, and HTTP QA publication.

## Phase 1 public surface

- `wire::CubicBezier` implements wire sampling, distance, knife intersection,
  and endpoint/body gesture priority.
- `selection` applies ordered logical click and marquee policies to arbitrary
  copyable IDs and resolves topmost descriptor hits.

The app converts its `RenderedEdge` and `SelectionTarget` values at this
boundary. No video-domain type crosses into this package. Unit tests use plain
integer IDs to keep that property executable.

## Deliberately deferred

The full per-frame descriptor and intent API (`GraphFrame`, `EditorOutput`,
connect/move/delete/reparent intents, host-rendered node bodies) is not declared
yet. It needs a fake in-memory host and the production adapter in the same
change so the API is driven by two consumers rather than guessed. Likewise,
the current layout algorithms still read `Project` directly and must first be
split into a domain-neutral graph ranking input plus a video-specific
projection.

Run `scripts/check-node-editor-ui-boundary.sh` to enforce the dependency
direction. The workspace quality gate runs it automatically.
