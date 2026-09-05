use super::super::export::cancellation::ExportCheckpoint;
use super::*;
use crate::AuthoringExportResult;

const SENTINEL: &[u8] = b"the original completed video";

fn submit(
    server: &RenderServer,
    project: &Arc<AuthoringProject>,
    plan: &Arc<RenderPlan>,
    id: u64,
    path: &Path,
) -> bool {
    server.send_authoring_video_export_request(
        RenderRequestId::new(id),
        Arc::clone(project),
        Arc::clone(plan),
        project.root_timeline_id,
        path.to_string_lossy().into_owned(),
    )
}

fn receive(server: &RenderServer) -> AuthoringExportResult {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match server.poll_authoring_export_result() {
            Ok(result) => return result,
            Err(TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("export did not complete: {error}"),
        }
    }
}

#[test]
fn accepted_cancellation_preserves_destination_and_cleans_before_single_completion() {
    for (checkpoint, frames, audio_files) in [
        (ExportCheckpoint::BeforeStart, 0, 0),
        (ExportCheckpoint::AudioWindow(0), 0, 1),
        (ExportCheckpoint::BeforeFrame(0), 0, 1),
        (ExportCheckpoint::FrameRendered(0), 0, 1),
        (ExportCheckpoint::BeforeFrame(1), 1, 1),
        (ExportCheckpoint::FrameRendered(1), 1, 1),
        (ExportCheckpoint::BeforePublication, 2, 1),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("movie.mp4");
        fs::write(&path, SENTINEL).unwrap();
        let (project, wave_path) = export_project_with_audio(directory.path());
        let (server, project, plan, probe) = export_server_for_project(ExportFault::None, project);
        let gate = server
            .cancellation_test_control
            .arm(checkpoint.clone())
            .unwrap();
        assert!(submit(&server, &project, &plan, 41, &path));
        gate.wait_reached().unwrap();
        // Duplicate request IDs never alias or replace the active token.
        assert!(!submit(&server, &project, &plan, 41, &path));
        assert!(!server.cancel_authoring_export_request(RenderRequestId::new(999)));
        assert!(server.cancel_authoring_export_request(RenderRequestId::new(41)));
        assert!(server.cancel_authoring_export_request(RenderRequestId::new(41)));
        gate.release();
        let result = receive(&server);
        assert_eq!(result.request_id, RenderRequestId::new(41));
        assert!(
            matches!(result.output, Err(LibraryError::ExportCancelled)),
            "{checkpoint:?}: {:?}",
            result.output
        );
        assert!(!result.published);
        assert_eq!(result.frames_exported, frames);
        assert_eq!(fs::read(&path).unwrap(), SENTINEL);
        assert_eq!(
            sibling_paths(directory.path(), &path),
            vec![wave_path.clone()]
        );
        assert_temporary_audio_cleaned(&server, audio_files, audio_files, 0, 0);
        assert_no_additional_completion(&server);
        assert!(!server.cancel_authoring_export_request(RenderRequestId::new(41)));
        let finishes = usize::from(frames > 0);
        {
            let probe = probe.lock().unwrap();
            assert_eq!(probe.frames, frames as usize);
            assert_eq!(probe.finishes, finishes);
        }

        // Consuming completion releases this ID, but does not poison the next
        // job, its Audio owner, or its logical output path.
        assert!(submit(&server, &project, &plan, 41, &path));
        let result = receive(&server);
        result.output.unwrap();
        assert!(result.published);
        assert_eq!(result.frames_exported, 2);
        assert_ne!(fs::read(&path).unwrap(), SENTINEL);
        assert_eq!(sibling_paths(directory.path(), &path), vec![wave_path]);
        assert_temporary_audio_cleaned(&server, audio_files + 1, audio_files + 1, 0, 0);
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, frames as usize + 2);
        assert_eq!(probe.finishes, finishes + 1);
        assert_no_additional_completion(&server);
    }
}

#[test]
fn cancellation_after_publication_starts_is_rejected_and_completion_is_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("movie.mp4");
    fs::write(&path, SENTINEL).unwrap();
    let (server, project, plan, probe) = export_server(ExportFault::None);
    let gate = server
        .cancellation_test_control
        .arm(ExportCheckpoint::PublicationStarted)
        .unwrap();
    assert!(submit(&server, &project, &plan, 42, &path));
    gate.wait_reached().unwrap();
    assert!(!server.cancel_authoring_export_request(RenderRequestId::new(42)));
    assert!(matches!(
        server.poll_authoring_export_result(),
        Err(TryRecvError::Empty)
    ));
    assert_eq!(fs::read(&path).unwrap(), SENTINEL);
    gate.release();
    let result = receive(&server);
    result.output.unwrap();
    assert!(result.published);
    assert_eq!(result.frames_exported, 2);
    assert_ne!(fs::read(&path).unwrap(), SENTINEL);
    assert!(sibling_paths(directory.path(), &path).is_empty());
    assert_eq!(probe.lock().unwrap().finishes, 1);
    assert_no_additional_completion(&server);
}

#[test]
fn drop_cancels_active_and_queued_jobs_and_joins_export_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("movie.mp4");
    let queued_path = directory.path().join("queued.mp4");
    fs::write(&path, SENTINEL).unwrap();
    fs::write(&queued_path, SENTINEL).unwrap();
    let (project, _) = export_project_with_audio(directory.path());
    let (mut server, project, plan, probe) = export_server_for_project(ExportFault::None, project);
    let gate = server
        .cancellation_test_control
        .arm(ExportCheckpoint::BeforeFrame(1))
        .unwrap();
    assert!(submit(&server, &project, &plan, 43, &path));
    gate.wait_reached().unwrap();
    assert!(submit(&server, &project, &plan, 44, &queued_path));
    let rejected_path = directory.path().join("rejected.mp4");
    assert!(!submit(&server, &project, &plan, 45, &rejected_path));
    assert!(!server.cancel_authoring_export_request(RenderRequestId::new(45)));
    let audio = Arc::clone(&server.temporary_audio_test_control);
    let (_unused_sender, unused_receiver) = std::sync::mpsc::channel();
    let completions = std::mem::replace(&mut server.rx_authoring_export_result, unused_receiver);
    let (finished, observe_finished) = std::sync::mpsc::channel();
    let dropping = std::thread::spawn(move || {
        drop(server);
        finished.send(()).unwrap();
    });
    assert!(matches!(
        observe_finished.recv_timeout(Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    gate.release();
    observe_finished
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    dropping.join().unwrap();
    for (id, frames) in [(43, 1), (44, 0)] {
        let result = completions.try_recv().unwrap();
        assert_eq!(result.request_id, RenderRequestId::new(id));
        assert!(matches!(result.output, Err(LibraryError::ExportCancelled)));
        assert_eq!(result.frames_exported, frames);
        assert!(!result.published);
    }
    assert!(matches!(
        completions.try_recv(),
        Err(TryRecvError::Disconnected)
    ));
    assert_eq!(fs::read(&path).unwrap(), SENTINEL);
    assert_eq!(fs::read(&queued_path).unwrap(), SENTINEL);
    assert!(!rejected_path.exists());
    // Exclude both user destinations and the input Audio file.
    let remaining = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(remaining.len(), 3);
    let (created, explicit, fallback, injected) = audio.observation();
    assert_eq!((created.len(), explicit, fallback, injected), (1, 1, 0, 0));
    assert!(created.iter().all(|path| !path.exists()));
    let probe = probe.lock().unwrap();
    assert_eq!((probe.frames, probe.finishes), (1, 1));
}

#[test]
fn cancelling_a_queued_png_does_not_cancel_the_active_video() {
    let directory = tempfile::tempdir().unwrap();
    let video_path = directory.path().join("movie.mp4");
    let png_path = directory.path().join("frame.png");
    fs::write(&png_path, SENTINEL).unwrap();
    let (server, project, plan, probe) = export_server(ExportFault::None);
    let gate = server
        .cancellation_test_control
        .arm(ExportCheckpoint::BeforeFrame(1))
        .unwrap();
    assert!(submit(&server, &project, &plan, 46, &video_path));
    gate.wait_reached().unwrap();
    assert!(server.send_authoring_png_export_request(
        RenderRequestId::new(47),
        Arc::clone(&project),
        Arc::clone(&plan),
        project.root_timeline_id,
        0,
        png_path.to_string_lossy().into_owned(),
    ));
    assert!(server.cancel_authoring_export_request(RenderRequestId::new(47)));
    gate.release();
    let video = receive(&server);
    assert_eq!(video.request_id, RenderRequestId::new(46));
    video.output.unwrap();
    assert!(video.published);
    let png = receive(&server);
    assert_eq!(png.request_id, RenderRequestId::new(47));
    assert!(matches!(png.output, Err(LibraryError::ExportCancelled)));
    assert!(!png.published);
    assert_eq!(png.frames_exported, 0);
    assert_eq!(fs::read(&png_path).unwrap(), SENTINEL);
    assert_eq!(sibling_paths(directory.path(), &video_path), vec![png_path]);
    assert_eq!(probe.lock().unwrap().finishes, 1);
    assert_no_additional_completion(&server);
}

#[test]
fn cancellation_keeps_finalization_and_cleanup_failures_visible() {
    for (fault, fail_audio_cleanup) in [(ExportFault::Finish, false), (ExportFault::None, true)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("movie.mp4");
        fs::write(&path, SENTINEL).unwrap();
        let (project, wave_path) = export_project_with_audio(directory.path());
        let (server, project, plan, probe) = export_server_for_project(fault, project);
        if fail_audio_cleanup {
            server.fail_temporary_audio_explicit_cleanup().unwrap();
        }
        let gate = server
            .cancellation_test_control
            .arm(ExportCheckpoint::BeforeFrame(1))
            .unwrap();
        assert!(submit(&server, &project, &plan, 48, &path));
        gate.wait_reached().unwrap();
        assert!(server.cancel_authoring_export_request(RenderRequestId::new(48)));
        gate.release();
        let result = receive(&server);
        let LibraryError::OperationAndCleanup {
            operation, cleanup, ..
        } = result.output.unwrap_err()
        else {
            panic!("cancellation must retain its secondary failure");
        };
        assert!(matches!(*operation, LibraryError::ExportCancelled));
        assert!(cleanup.to_string().contains(if fail_audio_cleanup {
            "cannot remove temporary authoring audio"
        } else {
            "injected exporter finish failure"
        }));
        assert!(!result.published);
        assert_eq!(result.frames_exported, 1);
        assert_eq!(probe.lock().unwrap().finishes, 1);
        assert_eq!(fs::read(&path).unwrap(), SENTINEL);
        assert_eq!(sibling_paths(directory.path(), &path), vec![wave_path]);
        let (audio_paths, ..) = server.temporary_audio_test_observation();
        assert_eq!(audio_paths.len(), 1);
        assert!(audio_paths.iter().all(|path| !path.exists()));
        assert_no_additional_completion(&server);
    }
}

#[test]
fn png_cancellation_obeys_its_direct_write_publication_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("frame.png");
    let project = export_project();
    let plan = Arc::new(RenderPlanCompiler::compile(&project).unwrap());
    let server = RenderServer::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    for (checkpoint, accepts_cancel) in [
        (ExportCheckpoint::BeforeStart, true),
        (ExportCheckpoint::BeforePublication, true),
        (ExportCheckpoint::PublicationStarted, false),
    ] {
        fs::write(&path, SENTINEL).unwrap();
        let gate = server.cancellation_test_control.arm(checkpoint).unwrap();
        assert!(server.send_authoring_png_export_request(
            RenderRequestId::new(49),
            Arc::clone(&project),
            Arc::clone(&plan),
            project.root_timeline_id,
            0,
            path.to_string_lossy().into_owned(),
        ));
        gate.wait_reached().unwrap();
        assert_eq!(
            server.cancel_authoring_export_request(RenderRequestId::new(49)),
            accepts_cancel
        );
        assert!(matches!(
            server.poll_authoring_export_result(),
            Err(TryRecvError::Empty)
        ));
        assert_eq!(fs::read(&path).unwrap(), SENTINEL);
        gate.release();
        let result = receive(&server);
        assert_eq!(result.request_id, RenderRequestId::new(49));
        assert_eq!(result.published, !accepts_cancel);
        if accepts_cancel {
            assert!(matches!(result.output, Err(LibraryError::ExportCancelled)));
            assert_eq!(result.frames_exported, 0);
            assert_eq!(fs::read(&path).unwrap(), SENTINEL);
        } else {
            result.output.unwrap();
            assert_eq!(result.frames_exported, 1);
            let image = image::open(&path).unwrap();
            assert_eq!((image.width(), image.height()), (4, 2));
        }
        assert!(sibling_paths(directory.path(), &path).is_empty());
        assert_no_additional_completion(&server);
    }
}
