use super::{ExportDialog, ExportUpdate, channel};
use library::cache::CacheManager;
use library::plugin::PluginManager;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

#[test]
fn failed_export_update_stops_progress_and_preserves_the_user_visible_error() {
    let mut dialog = ExportDialog::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    let (sender, receiver) = channel();
    dialog.is_exporting = true;
    dialog.progress_rx = Some(receiver);
    dialog.cancellation_token = Some(Arc::new(AtomicBool::new(false)));
    sender
        .send(ExportUpdate::Failed(
            "Export failed. Partial output may remain at test.mp4.".to_string(),
        ))
        .unwrap();

    dialog.poll_export_updates();

    assert!(!dialog.is_exporting);
    assert!(dialog.progress_rx.is_none());
    assert!(dialog.cancellation_token.is_none());
    assert!(dialog.status_message.contains("Partial output may remain"));
}

#[test]
fn cancellation_keeps_restart_blocked_until_worker_finalization_and_join() {
    let mut dialog = ExportDialog::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    let (sender, receiver) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let finalized = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let worker_finalized = Arc::clone(&finalized);
    dialog.is_exporting = true;
    dialog.progress_rx = Some(receiver);
    dialog.cancellation_token = Some(Arc::clone(&cancel));
    dialog.export_worker = Some(thread::spawn(move || {
        while !worker_cancel.load(Ordering::Relaxed) {
            thread::yield_now();
        }
        worker_finalized.store(true, Ordering::Relaxed);
        let _send_result = sender.send(ExportUpdate::Cancelled);
    }));

    dialog.request_cancel();
    assert!(dialog.is_exporting);
    assert!(!dialog.can_start_export());
    assert!(dialog.status_message.starts_with("Cancelling"));

    for _ in 0..10_000 {
        dialog.poll_export_updates();
        if !dialog.is_exporting {
            break;
        }
        thread::yield_now();
    }

    assert!(finalized.load(Ordering::Relaxed));
    assert!(dialog.export_worker.is_none());
    assert!(dialog.can_start_export());
    assert_eq!(dialog.status_message, "Cancelled.");
}

#[test]
#[allow(
    clippy::panic,
    reason = "the test verifies worker panic detection and join"
)]
fn worker_panic_is_joined_and_reported_as_failure() {
    let mut dialog = ExportDialog::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    let (_sender, receiver) = channel();
    dialog.is_exporting = true;
    dialog.progress_rx = Some(receiver);
    dialog.export_worker = Some(thread::spawn(|| panic!("intentional worker panic")));

    for _ in 0..10_000 {
        dialog.poll_export_updates();
        if !dialog.is_exporting {
            break;
        }
        thread::yield_now();
    }

    assert!(!dialog.is_exporting);
    assert!(dialog.export_worker.is_none());
    assert!(dialog.status_message.contains("worker panicked"));
}
