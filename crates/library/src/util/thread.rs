//! Thread lifecycle helpers shared by asynchronous runtimes.

use std::thread::JoinHandle;

/// Reaps worker threads without making the caller's thread wait for in-flight
/// media IO, decode, rendering, or export work.
///
/// Dropping a `JoinHandle` already detaches its worker. The small named reaper
/// keeps normal in-process teardown deterministic while preserving that
/// non-blocking behavior if the reaper itself cannot be started.
pub(crate) fn join_in_background(
    reaper_name: &'static str,
    workers: Vec<(&'static str, JoinHandle<()>)>,
) {
    if workers.is_empty() {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name(reaper_name.to_string())
        .spawn(move || {
            for (worker_name, worker) in workers {
                if worker.join().is_err() {
                    log::error!("{worker_name} panicked during shutdown");
                }
            }
        })
    {
        log::error!("Failed to start {reaper_name}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_does_not_wait_for_the_worker() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _wait_result = wait.recv();
        });
        let (returned, observe_return) = std::sync::mpsc::channel();
        let caller = std::thread::spawn(move || {
            join_in_background("test-worker-reaper", vec![("test worker", worker)]);
            let _send_result = returned.send(());
        });
        assert!(
            observe_return
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_ok()
        );
        drop(release);
        caller.join().unwrap();
    }
}
