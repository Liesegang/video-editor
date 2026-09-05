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
Core also pins one exporter endpoint for the whole job. Replacing a registry
entry while a callback is running affects the next job, never the remaining
frames or finalization of the active job.
The exporter closes and waits for its encoder, then Core cleans temporary
Audio and synchronizes the staging file. While the reserved staging handle is
still open, Core revalidates its non-empty regular-file identity, every source
alias, and the final destination identity; it then closes the handle and
atomically replaces the destination. Explicit output validation permits Windows
UNC shares while retaining the automatic-media ban on network locators. Any
earlier render, effect, frame-write, encoder,
cleanup, validation, or publication failure preserves an existing destination
and attempts to remove the staging file. Once temporary Audio ownership has
been created, the coordinator owns it before rendering or writing any samples.
Normal completion, preparation errors, frame errors, and unwinding panics all
therefore return to one explicit cleanup path. Transient interrupted, sharing,
or permission failures receive a bounded retry before publication. If cleanup
still fails, publication is rejected and the cleanup error is reported together
with any primary failure in a typed `LibraryError::OperationAndCleanup`; `Drop`
performs a final bounded fallback rather than being the normal cleanup path. A
permanent operating-system failure can still leave the exact temporary path
behind and remains visible in the error log.
`frames_exported` counts frames accepted
before a failure; `published` becomes true only after atomic replacement.
Failed exports emit one terminal failure completion so the app can clear its
pending state, replace the stale `Exporting ...` status with `Export failed for
...`, and surface the concrete failure; they never emit a success or published
update. Starting a retry clears only the error previously reported by Export,
so an unrelated subsystem error is neither hidden nor mistaken for Export
progress.

Each accepted Export request also owns one cancellation state, addressed by
`RenderRequestId` until its completion is polled. Duplicate outstanding IDs are
rejected. `cancel_authoring_export_request` returns whether cancellation was
accepted, not whether cleanup has finished. The worker checks that same state
before work, during Particle preflight and Audio windows, before frame
evaluation, and between rendering and exporter writes. Cancellation follows
the ordinary finalization/cleanup path: an unattempted exporter is not finished;
an attempted exporter is finished exactly once. A clean cancellation emits one
typed `LibraryError::ExportCancelled` completion with `published = false`.
Finalization or cleanup errors remain visible alongside cancellation in the
typed combined error; cancellation does not erase an earlier operation failure.

After exporter finalization and Audio cleanup, an atomic state transition
closes the cancellation window before publication begins. An accepted cancel
cannot publish. Once this transition wins, further cancellation returns false
and callers wait for sync/validation/atomic replacement to finish. PNG export,
which writes directly, closes its cancellation window before its exporter
write instead; it does not acquire Video's atomic-publication guarantees.

Approved New/Open/Quit actions retain the current project and pending Export
until the matching terminal completion arrives. Cancelling the unsaved-changes
prompt does not cancel Export. Ordinary cancellation is shown as status, not
an error dialog; combined cleanup failures remain errors. Window close stays
uncommitted while cleanup is pending, and project replacement does not reset
the request ID sequence. As a final normal-shutdown guarantee, `RenderServer`
Drop cancels active and queued exports and joins the export worker synchronously.
Preview retains its separate non-blocking teardown. Cancellation is cooperative:
an in-flight plugin callback, media decode, GPU call, encoder write or encoder
wait must return before the worker can observe it. This is not a bounded-time
shutdown or a process-kill cleanup guarantee.

Focused production-path regressions execute a non-zero built-in Blur
Attachment and the real FFmpeg Video Loader before a private, test-only
one-shot hook converts their successful callback into a failure. Ordinary
errors are covered on both the first and second frame. A first-frame failure
does not call an exporter and therefore does not finalize one; after an earlier
frame has been accepted, the pinned exporter is finalized exactly once. These
checks preserve an existing destination byte-for-byte, remove staging output,
emit one failed completion, and allow the same `RenderServer` to complete the
next request. The second-frame cases with an Audio route also verify removal of
the generated temporary Audio file.

A separate unwind injected after a successful second-frame FFmpeg load verifies
that Audio and staging cleanup still run and that the worker accepts the next
request after discarding the renderer. This test does not claim that a renderer
survives a panic; ordinary `LibraryError` and unwinding panic deliberately have
different renderer-lifetime contracts.

An unwinding panic in the video job body is converted to an error before the
same pinned exporter is finalized exactly once. Finalization has its own panic
guard; Audio and staging cleanup still run, the renderer is discarded only
after cleanup, and the worker remains available for the next request. An outer
request guard supplies a terminal failure completion for residual panics. This
does not claim recovery from `panic=abort`, OOM abort, FFI abort, process kill,
or power loss.

`RenderServer` is the sole production authoring export coordinator. The removed
pre-v1 `editor::ExportService` direct-final-write path is not retained as a
compatibility API. Project save and production authoring video Export reuse the
same atomic-file primitive instead of carrying separate platform replacement
implementations.

The shared primitive also owns the only staging `sync_all` operation. Its
instance-local test control can fail exactly that boundary without adding a
second RenderServer publication implementation. Regression tests let a valid
two-frame export and its pinned exporter finalization finish, inject the sync
failure, then verify one failed completion, `published = false`, an unchanged
destination, no sibling staging file, and a successful retry on the same
worker. On Windows a separate test keeps the destination open with read/write
sharing but deliberately omits delete sharing. Initial and final identity
checks therefore still succeed, while the real
`MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` replacement is rejected with
`ERROR_ACCESS_DENIED` or `ERROR_SHARING_VIOLATION`; releasing that handle makes
the same-path retry succeed. These checks cover publication visibility, not
parent-directory synchronization or power-loss durability.

The focused regression checks are:

```sh
cargo test -p library core::rendering -- --nocapture
cargo test -p library util::atomic_file -- --nocapture
cargo test -p library publication_failure_tests -- --nocapture
cargo test -p library cancellation -- --nocapture
cargo test -p app failed_export_completion_clears_pending_status_and_surfaces_error -- --nocapture
cargo test -p app unpublished_export_completion_clears_pending_status_and_surfaces_error -- --nocapture
cargo test -p app retry_after_failed_export_clears_only_the_previous_export_error -- --nocapture
cargo test -p library production_blur_frame_zero_failure_is_atomic_and_the_same_server_recovers -- --nocapture
cargo test -p library audio_preparation_and_cleanup_failures_are_both_reported_before_frame_zero -- --nocapture
python scripts/qa-runner.py --mode full --jobs 1
```
