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
Production authoring video Export never opens the user-selected destination
while work is in progress. Core reserves a same-directory staging file and
passes exporters an explicit logical destination plus writable staging path.
The logical destination is leased from before staging creation until publish or
abort, independently of the encoder session, so another in-process coordinator
cannot enter the finish-to-rename gap. Core records the initial filesystem
identity of the destination and the reserved staging file and rejects either
path if its directory entry changes during the job.
The exporter closes and waits for its encoder, then Core cleans temporary
Audio and synchronizes the staging file. While the reserved staging handle is
still open, Core revalidates its non-empty regular-file identity, every source
alias, and the final destination identity; it then closes the handle and
atomically replaces the destination. Explicit output validation permits Windows
UNC shares while retaining the automatic-media ban on network locators. Any
earlier render, effect, frame-write, encoder,
cleanup, validation, or publication failure preserves an existing destination
and attempts to remove the staging file; a cleanup failure is included in the
reported error rather than hidden. `frames_exported` counts frames accepted
before a failure; `published` becomes true only after atomic replacement.
Failed exports emit one terminal failure completion so the app can clear its
pending state, but never emit a success or published update.

`RenderServer` is the sole production authoring export coordinator. The removed
pre-v1 `editor::ExportService` direct-final-write path is not retained as a
compatibility API. Project save and production authoring video Export reuse the
same atomic-file primitive instead of carrying separate platform replacement
implementations.

The focused regression checks are:

```sh
cargo test -p library core::rendering -- --nocapture
cargo test -p library util::atomic_file -- --nocapture
cargo test -p app render_error_invalidates_stale_output -- --nocapture
cargo test -p app failed_export_update_stops_progress -- --nocapture
python scripts/qa-runner.py --mode full --jobs 1
```
