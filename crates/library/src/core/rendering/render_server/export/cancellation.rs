//! Request-local cancellation state for authoring Export jobs.
//!
//! Cancellation remains an internal RenderServer contract. A request may be
//! cancelled while it is running, but publication is an indivisible terminal
//! boundary: once publication begins, late cancellation is rejected.

use std::collections::{HashMap, hash_map::Entry};
#[cfg(test)]
use std::sync::Condvar;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
#[cfg(test)]
use std::time::Duration;

use crate::error::LibraryError;

use super::super::RenderRequestId;

const RUNNING: u8 = 0;
const CANCELLED: u8 = 1;
const PUBLISHING: u8 = 2;
const FINISHED: u8 = 3;

#[cfg(test)]
const TEST_GATE_TIMEOUT: Duration = Duration::from_secs(10);

/// Deterministic worker checkpoints available only to in-crate tests.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::core::rendering::render_server) enum ExportCheckpoint {
    BeforeStart,
    BeforeFrame(u64),
    FrameRendered(u64),
    AudioWindow(u64),
    BeforePublication,
    PublicationStarted,
}

#[cfg(test)]
#[derive(Default)]
struct TestGateState {
    reached: bool,
    released: bool,
}

#[cfg(test)]
#[derive(Default)]
struct TestGate {
    state: Mutex<TestGateState>,
    changed: Condvar,
}

#[cfg(test)]
impl TestGate {
    fn reach_and_wait(&self) -> Result<(), LibraryError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.reached = true;
        self.changed.notify_all();
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, TEST_GATE_TIMEOUT, |state| !state.released)
            .unwrap_or_else(PoisonError::into_inner);
        if timeout.timed_out() && !state.released {
            return Err(LibraryError::Runtime(
                "Timed out waiting to release an Export cancellation test checkpoint".to_string(),
            ));
        }
        Ok(())
    }

    fn wait_reached(&self) -> Result<(), LibraryError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, TEST_GATE_TIMEOUT, |state| !state.reached)
            .unwrap_or_else(PoisonError::into_inner);
        if timeout.timed_out() && !state.reached {
            return Err(LibraryError::Runtime(
                "Timed out waiting for an Export cancellation test checkpoint".to_string(),
            ));
        }
        Ok(())
    }

    fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.released = true;
        self.changed.notify_all();
    }
}

#[cfg(test)]
struct ArmedCheckpoint {
    checkpoint: ExportCheckpoint,
    gate: Arc<TestGate>,
}

/// Instance-local, one-shot synchronization control for cancellation tests.
#[cfg(test)]
#[derive(Default)]
pub(in crate::core::rendering::render_server) struct CancellationTestControl {
    armed: Mutex<Option<ArmedCheckpoint>>,
}

#[cfg(test)]
impl CancellationTestControl {
    pub(in crate::core::rendering::render_server) fn arm(
        &self,
        checkpoint: ExportCheckpoint,
    ) -> Result<CancellationTestGate, LibraryError> {
        let mut armed = self.armed.lock().unwrap_or_else(PoisonError::into_inner);
        if armed.is_some() {
            return Err(LibraryError::Runtime(
                "An Export cancellation test checkpoint is already armed".to_string(),
            ));
        }
        let gate = Arc::new(TestGate::default());
        *armed = Some(ArmedCheckpoint {
            checkpoint,
            gate: Arc::clone(&gate),
        });
        Ok(CancellationTestGate { gate })
    }

    fn pause_at(&self, checkpoint: &ExportCheckpoint) -> Result<(), LibraryError> {
        let gate = {
            let mut armed = self.armed.lock().unwrap_or_else(PoisonError::into_inner);
            if armed
                .as_ref()
                .is_some_and(|armed| &armed.checkpoint == checkpoint)
            {
                armed.take().map(|armed| armed.gate)
            } else {
                None
            }
        };
        gate.map_or(Ok(()), |gate| gate.reach_and_wait())
    }
}

/// Test-side handle for observing and releasing one worker checkpoint.
#[cfg(test)]
pub(in crate::core::rendering::render_server) struct CancellationTestGate {
    gate: Arc<TestGate>,
}

#[cfg(test)]
impl CancellationTestGate {
    pub(in crate::core::rendering::render_server) fn wait_reached(
        &self,
    ) -> Result<(), LibraryError> {
        self.gate.wait_reached()
    }

    pub(in crate::core::rendering::render_server) fn release(&self) {
        self.gate.release();
    }
}

#[cfg(test)]
impl Drop for CancellationTestGate {
    fn drop(&mut self) {
        self.gate.release();
    }
}

/// One Export request's lock-free cancellation/publication state machine.
pub(in crate::core::rendering::render_server) struct ExportCancellation {
    state: AtomicU8,
    #[cfg(test)]
    test_control: Mutex<Option<Arc<CancellationTestControl>>>,
}

impl Default for ExportCancellation {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(RUNNING),
            #[cfg(test)]
            test_control: Mutex::new(None),
        }
    }
}

impl ExportCancellation {
    /// Request cancellation before publication starts.
    ///
    /// Repeating an accepted cancellation is idempotently successful. Once
    /// publication or terminal completion begins, cancellation is too late.
    pub(in crate::core::rendering::render_server) fn request_cancel(&self) -> bool {
        loop {
            match self.state.load(Ordering::SeqCst) {
                RUNNING => {
                    if self
                        .state
                        .compare_exchange(RUNNING, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        return true;
                    }
                }
                CANCELLED => return true,
                PUBLISHING | FINISHED => return false,
                _ => return false,
            }
        }
    }

    /// Fail the current operation when cancellation has been accepted.
    pub(in crate::core::rendering::render_server) fn check(&self) -> Result<(), LibraryError> {
        match self.state.load(Ordering::SeqCst) {
            CANCELLED => Err(LibraryError::ExportCancelled),
            RUNNING | PUBLISHING | FINISHED => Ok(()),
            _ => Err(invalid_state()),
        }
    }

    /// Atomically close the cancellation window and enter publication.
    pub(in crate::core::rendering::render_server) fn begin_publication(
        &self,
    ) -> Result<(), LibraryError> {
        match self
            .state
            .compare_exchange(RUNNING, PUBLISHING, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => Ok(()),
            Err(CANCELLED) => Err(LibraryError::ExportCancelled),
            Err(PUBLISHING) => Err(LibraryError::Runtime(
                "Export publication has already started".to_string(),
            )),
            Err(FINISHED) => Err(LibraryError::Runtime(
                "Export request has already finished".to_string(),
            )),
            Err(_) => Err(invalid_state()),
        }
    }

    /// Mark every terminal request outcome as finished.
    pub(in crate::core::rendering::render_server) fn finish(&self) {
        self.state.store(FINISHED, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(in crate::core::rendering::render_server) fn set_test_control(
        &self,
        control: Arc<CancellationTestControl>,
    ) -> Result<(), LibraryError> {
        let mut installed = self
            .test_control
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if installed.is_some() {
            return Err(LibraryError::Runtime(
                "Export cancellation test control is already installed".to_string(),
            ));
        }
        *installed = Some(control);
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::core::rendering::render_server) fn pause_at(
        &self,
        checkpoint: ExportCheckpoint,
    ) -> Result<(), LibraryError> {
        let control = self
            .test_control
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        control.map_or(Ok(()), |control| control.pause_at(&checkpoint))
    }
}

fn invalid_state() -> LibraryError {
    LibraryError::Runtime("Export cancellation state is invalid".to_string())
}

/// RenderServer-owned lookup for live Export cancellation state.
#[derive(Default)]
pub(in crate::core::rendering::render_server) struct ExportCancellations {
    requests: Mutex<HashMap<RenderRequestId, Arc<ExportCancellation>>>,
}

impl ExportCancellations {
    /// Register a fresh request, rejecting a duplicate live request identity.
    pub(in crate::core::rendering::render_server) fn register(
        &self,
        request_id: RenderRequestId,
    ) -> Option<Arc<ExportCancellation>> {
        let mut requests = self.requests();
        match requests.entry(request_id) {
            Entry::Occupied(_) => None,
            Entry::Vacant(entry) => {
                let cancellation = Arc::new(ExportCancellation::default());
                entry.insert(Arc::clone(&cancellation));
                Some(cancellation)
            }
        }
    }

    /// Remove a completed or rejected request from the live registry.
    pub(in crate::core::rendering::render_server) fn remove(&self, request_id: RenderRequestId) {
        self.requests().remove(&request_id);
    }

    /// Cancel one live request without retaining the registry lock.
    pub(in crate::core::rendering::render_server) fn cancel(
        &self,
        request_id: RenderRequestId,
    ) -> bool {
        let cancellation = self.requests().get(&request_id).cloned();
        cancellation.is_some_and(|cancellation| cancellation.request_cancel())
    }

    /// Cancel every live request without invoking state transitions under the
    /// registry lock.
    pub(in crate::core::rendering::render_server) fn cancel_all(&self) {
        let cancellations = self.requests().values().cloned().collect::<Vec<_>>();
        for cancellation in cancellations {
            cancellation.request_cancel();
        }
    }

    fn requests(&self) -> MutexGuard<'_, HashMap<RenderRequestId, Arc<ExportCancellation>>> {
        self.requests.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_states_are_terminal_and_idempotent() {
        let cancellation = ExportCancellation::default();
        assert!(cancellation.check().is_ok());
        assert!(cancellation.request_cancel());
        assert!(cancellation.request_cancel());
        assert!(matches!(
            cancellation.check(),
            Err(LibraryError::ExportCancelled)
        ));

        cancellation.finish();
        assert!(!cancellation.request_cancel());
    }

    #[test]
    fn accepted_cancellation_cannot_enter_publication() {
        let cancellation = ExportCancellation::default();
        assert!(cancellation.request_cancel());
        assert!(matches!(
            cancellation.begin_publication(),
            Err(LibraryError::ExportCancelled)
        ));
    }

    #[test]
    fn publication_rejects_late_cancellation() {
        let cancellation = ExportCancellation::default();
        cancellation.begin_publication().unwrap();
        assert!(!cancellation.request_cancel());
        assert!(cancellation.check().is_ok());

        cancellation.finish();
        assert!(!cancellation.request_cancel());
        assert!(matches!(
            cancellation.begin_publication(),
            Err(LibraryError::Runtime(_))
        ));
    }

    #[test]
    fn registry_rejects_duplicate_live_request_ids() {
        let registry = ExportCancellations::default();
        let request_id = RenderRequestId::new(71);

        let cancellation = registry.register(request_id).unwrap();
        assert!(registry.register(request_id).is_none());
        assert!(registry.cancel(request_id));
        assert!(matches!(
            cancellation.check(),
            Err(LibraryError::ExportCancelled)
        ));
    }

    #[test]
    fn unregister_removes_only_the_registry_reference() {
        let registry = ExportCancellations::default();
        let request_id = RenderRequestId::new(72);
        let cancellation = registry.register(request_id).unwrap();

        registry.remove(request_id);

        assert!(!registry.cancel(request_id));
        assert!(cancellation.check().is_ok());
        assert!(registry.register(request_id).is_some());
    }

    #[test]
    fn cancel_all_transitions_each_registered_running_request() {
        let registry = ExportCancellations::default();
        let first = registry.register(RenderRequestId::new(73)).unwrap();
        let second = registry.register(RenderRequestId::new(74)).unwrap();

        registry.cancel_all();

        assert!(matches!(first.check(), Err(LibraryError::ExportCancelled)));
        assert!(matches!(second.check(), Err(LibraryError::ExportCancelled)));
    }

    #[test]
    fn armed_checkpoints_block_once_and_release_explicitly_or_on_drop() {
        let control = Arc::new(CancellationTestControl::default());
        let cancellation = Arc::new(ExportCancellation::default());
        cancellation.set_test_control(Arc::clone(&control)).unwrap();
        let gate = control.arm(ExportCheckpoint::BeforeFrame(3)).unwrap();
        let worker_cancellation = Arc::clone(&cancellation);
        let worker = std::thread::spawn(move || {
            worker_cancellation.pause_at(ExportCheckpoint::BeforeFrame(3))
        });

        gate.wait_reached().unwrap();
        gate.release();
        worker.join().unwrap().unwrap();

        let gate = control.arm(ExportCheckpoint::BeforeFrame(4)).unwrap();
        let worker_cancellation = Arc::clone(&cancellation);
        let worker = std::thread::spawn(move || {
            worker_cancellation.pause_at(ExportCheckpoint::BeforeFrame(4))
        });
        gate.wait_reached().unwrap();
        drop(gate);
        worker.join().unwrap().unwrap();

        cancellation
            .pause_at(ExportCheckpoint::BeforeFrame(4))
            .unwrap();
    }
}
