use super::*;

const SENTINEL: &[u8] = b"keep this existing video";

fn assert_failed_publication(
    result: super::super::AuthoringExportResult,
    request_id: u64,
    final_path: &Path,
) -> String {
    let error = result.output.unwrap_err().to_string();
    assert_eq!(result.request_id, RenderRequestId::new(request_id));
    assert_eq!(result.frames_exported, 2);
    assert_eq!(result.frame_count, 2);
    assert!(!result.published);
    assert_eq!(fs::read(final_path).unwrap(), SENTINEL);
    assert!(sibling_paths(final_path.parent().unwrap(), final_path).is_empty());
    error
}

fn assert_successful_retry(
    result: super::super::AuthoringExportResult,
    request_id: u64,
    final_path: &Path,
) {
    result.output.unwrap();
    assert_eq!(result.request_id, RenderRequestId::new(request_id));
    assert_eq!(result.frames_exported, 2);
    assert_eq!(result.frame_count, 2);
    assert!(result.published);
    assert_ne!(fs::read(final_path).unwrap(), SENTINEL);
    assert!(sibling_paths(final_path.parent().unwrap(), final_path).is_empty());
}

fn assert_probe_completed_jobs(probe: &Arc<Mutex<ExportProbe>>, jobs: usize) {
    let probe = probe.lock().unwrap();
    assert_eq!(probe.frames, jobs * 2);
    assert_eq!(probe.finishes, jobs);
}

#[test]
fn sync_failure_is_terminal_and_same_worker_retry_publishes() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("sync-failure.mp4");
    fs::write(&final_path, SENTINEL).unwrap();
    let (server, project, plan, probe) = export_server(ExportFault::None);
    server.fail_next_atomic_file_sync().unwrap();

    let failed = request_export(&server, &project, &plan, 9_500, &final_path);
    let error = assert_failed_publication(failed, 9_500, &final_path);
    assert!(
        error.contains("injected atomic staging sync_all failure"),
        "{error}"
    );
    assert_probe_completed_jobs(&probe, 1);
    assert_eq!(server.atomic_sync_test_observation(), (1, 1));
    assert_no_additional_completion(&server);

    let retry = request_export(&server, &project, &plan, 9_501, &final_path);
    assert_successful_retry(retry, 9_501, &final_path);
    assert_probe_completed_jobs(&probe, 2);
    assert_eq!(server.atomic_sync_test_observation(), (2, 1));
    assert_no_additional_completion(&server);
}

#[cfg(windows)]
#[test]
fn windows_delete_share_denial_blocks_real_replace_and_retry_publishes() {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("sharing-violation.mp4");
    fs::write(&final_path, SENTINEL).unwrap();
    let destination_lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&final_path)
        .unwrap();
    let (server, project, plan, probe) = export_server(ExportFault::None);

    let failed = request_export(&server, &project, &plan, 9_510, &final_path);
    let error = assert_failed_publication(failed, 9_510, &final_path);
    assert!(error.contains("cannot atomically publish"), "{error}");
    assert!(
        error.contains("(os error 5)") || error.contains("(os error 32)"),
        "{error}"
    );
    assert_probe_completed_jobs(&probe, 1);
    assert_eq!(server.atomic_sync_test_observation(), (1, 0));
    assert_no_additional_completion(&server);

    drop(destination_lock);
    let retry = request_export(&server, &project, &plan, 9_511, &final_path);
    assert_successful_retry(retry, 9_511, &final_path);
    assert_probe_completed_jobs(&probe, 2);
    assert_eq!(server.atomic_sync_test_observation(), (2, 0));
    assert_no_additional_completion(&server);
}
