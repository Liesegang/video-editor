use super::*;

const SENTINEL: &[u8] = b"keep this existing video";

fn assert_published(result: super::super::AuthoringExportResult, frames: u64) {
    result.output.unwrap();
    assert_eq!(result.frames_exported, frames);
    assert_eq!(result.frame_count, 2);
    assert!(result.published);
}

fn assert_combined_audio_cleanup_failure(error: &LibraryError, operation_message: &str) {
    let LibraryError::OperationAndCleanup {
        operation_phase,
        operation,
        cleanup_phase,
        cleanup,
    } = error
    else {
        panic!("expected a structured operation/cleanup error, got {error}");
    };
    assert_eq!(*operation_phase, "video export failed");
    assert!(
        operation.to_string().contains(operation_message),
        "{operation}"
    );
    assert_eq!(*cleanup_phase, "temporary audio cleanup also failed");
    assert!(
        cleanup
            .to_string()
            .contains("injected temporary authoring audio cleanup failure"),
        "{cleanup}"
    );
}

#[test]
fn transient_temporary_audio_cleanup_failure_retries_before_publication() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("transient-cleanup.mp4");
    let (project, wave_path) = export_project_with_audio(directory.path());
    let (server, project, plan, probe) = export_server_for_project(ExportFault::None, project);
    server.fail_temporary_audio_cleanup_attempts(1).unwrap();

    let first = request_export(&server, &project, &plan, 9_400, &final_path);
    assert_published(first, 2);
    assert_eq!(
        sibling_paths(directory.path(), &final_path),
        vec![wave_path.clone()]
    );
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 2);
        assert_eq!(probe.finishes, 1);
    }
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 1, 2, 0, 1);

    let retry = request_export(&server, &project, &plan, 9_401, &final_path);
    assert_published(retry, 2);
    assert_eq!(
        sibling_paths(directory.path(), &final_path),
        vec![wave_path]
    );
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 4);
        assert_eq!(probe.finishes, 2);
    }
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 2, 3, 0, 1);
}

#[test]
fn persistent_temporary_audio_cleanup_failure_prevents_publication_then_drop_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("persistent-cleanup.mp4");
    fs::write(&final_path, SENTINEL).unwrap();
    let (project, wave_path) = export_project_with_audio(directory.path());
    let (server, project, plan, probe) = export_server_for_project(ExportFault::None, project);
    server.fail_temporary_audio_explicit_cleanup().unwrap();

    let failed = request_export(&server, &project, &plan, 9_410, &final_path);
    let error = failed.output.unwrap_err().to_string();
    assert!(
        error.contains("injected temporary authoring audio cleanup failure"),
        "{error}"
    );
    assert_eq!(failed.frames_exported, 2);
    assert_eq!(failed.frame_count, 2);
    assert!(!failed.published);
    assert_eq!(fs::read(&final_path).unwrap(), SENTINEL);
    assert_eq!(
        sibling_paths(directory.path(), &final_path),
        vec![wave_path.clone()]
    );
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 2);
        assert_eq!(probe.finishes, 1);
    }
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 1, 4, 1, 4);

    let retry = request_export(&server, &project, &plan, 9_411, &final_path);
    assert_published(retry, 2);
    assert_ne!(fs::read(&final_path).unwrap(), SENTINEL);
    assert_eq!(
        sibling_paths(directory.path(), &final_path),
        vec![wave_path]
    );
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 4);
        assert_eq!(probe.finishes, 2);
    }
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 2, 5, 1, 4);
}

#[test]
fn export_and_temporary_audio_cleanup_failures_are_both_reported() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("combined-cleanup.mp4");
    fs::write(&final_path, SENTINEL).unwrap();
    let (project, wave_path) = export_project_with_audio(directory.path());
    let (server, project, plan, probe) = export_server_for_project(ExportFault::Frame(0), project);
    server.fail_temporary_audio_explicit_cleanup().unwrap();

    let failed = request_export(&server, &project, &plan, 9_420, &final_path);
    let error = failed.output.unwrap_err();
    assert_combined_audio_cleanup_failure(&error, "injected frame 0 failure");
    assert_eq!(failed.frames_exported, 0);
    assert!(!failed.published);
    assert_eq!(fs::read(&final_path).unwrap(), SENTINEL);
    assert_eq!(
        sibling_paths(directory.path(), &final_path),
        vec![wave_path.clone()]
    );
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 1);
        assert_eq!(probe.finishes, 1);
    }
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 1, 4, 1, 4);

    let retry = request_export(&server, &project, &plan, 9_421, &final_path);
    assert_published(retry, 2);
    assert_eq!(
        sibling_paths(directory.path(), &final_path),
        vec![wave_path]
    );
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 3);
        assert_eq!(probe.finishes, 2);
    }
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 2, 5, 1, 4);
}

#[test]
fn audio_preparation_and_cleanup_failures_are_both_reported_before_frame_zero() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("preparation-cleanup.mp4");
    let source_path = directory.path().join("missing-then-restored.wav");
    fs::write(&final_path, SENTINEL).unwrap();
    let mut project = base_export_project();
    add_audio_item(&mut project, &source_path, 4_000);
    project.validate().unwrap();
    let project = Arc::new(project);
    let (server, project, plan, probe) = export_server_for_project(ExportFault::None, project);
    server.fail_temporary_audio_explicit_cleanup().unwrap();

    let failed = request_export(&server, &project, &plan, 9_430, &final_path);
    let error = failed.output.unwrap_err();
    assert_combined_audio_cleanup_failure(&error, "authoring audio render failed");
    assert_eq!(failed.frames_exported, 0);
    assert_eq!(probe.lock().unwrap().frames, 0);
    assert_eq!(probe.lock().unwrap().finishes, 0);
    assert!(!failed.published);
    assert_eq!(fs::read(&final_path).unwrap(), SENTINEL);
    assert!(sibling_paths(directory.path(), &final_path).is_empty());
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 1, 4, 1, 4);

    let source = vec![[0.25_f32, -0.25_f32]; 4_000];
    write_stereo_wave(&source_path, &source);
    let retry = request_export(&server, &project, &plan, 9_431, &final_path);
    assert_published(retry, 2);
    assert_eq!(
        sibling_paths(directory.path(), &final_path),
        vec![source_path]
    );
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 2);
        assert_eq!(probe.finishes, 1);
    }
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 2, 5, 1, 4);
}
