# Render error handling

Rendering APIs return `LibraryError` instead of panicking. In particular,
`SkiaRenderer::new` returns `Result<SkiaRenderer, LibraryError>`, and shared GPU
context replacement commits only after its replacement surface has been created.
If replacement fails, the previous context, surface, and sharing metadata remain
unchanged.

`RenderServer` sends every completed request back as a `RenderResult`. Its
`output` field is a `Result<RenderOutput, LibraryError>` so asynchronous surface
creation and frame rendering failures are not mistaken for a missing or stale
frame. Preview behavior is:

- a render failure clears the previous CPU/GPU texture and shows the original
  `LibraryError` through the existing modal error path;
- an error does not advance the preview render revision;
- the next successful render clears only the render error owned by Preview and
  installs the new pixels;
- unrelated modal errors are preserved.

Export progress uses explicit progress, complete, and failed terminal updates.
Initialization failures report that no output was written. Failures after
rendering starts report that partial output may remain and include its path in
the export dialog. Failed exports never emit a completion update.

The focused regression checks are:

```sh
cargo test -p library core::rendering -- --nocapture
cargo test -p app render_error_invalidates_stale_output -- --nocapture
cargo test -p app failed_export_update_stops_progress -- --nocapture
```
