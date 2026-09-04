use std::sync::{Condvar, Mutex, MutexGuard};

/// Bounded worker inbox for interactive Preview rendering.
///
/// Preview has latest-value semantics: while one frame is being rendered,
/// retaining every intermediate Project snapshot only increases latency and
/// memory use. The inbox therefore owns at most one pending frame and one
/// pending sharing-context update. Export uses its separate lossless queue.
pub(super) struct PreviewMailbox<T> {
    state: Mutex<PreviewMailboxState<T>>,
    ready: Condvar,
}

struct PreviewMailboxState<T> {
    latest_render: Option<T>,
    latest_sharing_context: Option<(usize, Option<isize>)>,
    shutdown: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewSubmission {
    Accepted,
    ReplacedPending,
    Closed,
}

pub(super) enum PreviewWorkerMessage<T> {
    Render(T),
    SetSharingContext(usize, Option<isize>),
    Shutdown,
}

impl<T> PreviewMailbox<T> {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(PreviewMailboxState {
                latest_render: None,
                latest_sharing_context: None,
                shutdown: false,
            }),
            ready: Condvar::new(),
        }
    }

    pub(super) fn submit_render(&self, render: T) -> PreviewSubmission {
        let mut state = self.lock_state();
        if state.shutdown {
            return PreviewSubmission::Closed;
        }
        let replaced = state.latest_render.replace(render).is_some();
        drop(state);
        self.ready.notify_one();
        if replaced {
            PreviewSubmission::ReplacedPending
        } else {
            PreviewSubmission::Accepted
        }
    }

    pub(super) fn set_sharing_context(&self, handle: usize, hwnd: Option<isize>) -> bool {
        let mut state = self.lock_state();
        if state.shutdown {
            return false;
        }
        state.latest_sharing_context = Some((handle, hwnd));
        drop(state);
        self.ready.notify_one();
        true
    }

    /// Take a newer frame that arrived after the worker woke but before
    /// evaluation began. This closes the small dequeue-to-render stale window
    /// without draining or cloning an unbounded request queue.
    pub(super) fn take_newer_render(&self) -> Option<T> {
        let mut state = self.lock_state();
        if state.shutdown || state.latest_sharing_context.is_some() {
            return None;
        }
        state.latest_render.take()
    }

    pub(super) fn recv(&self) -> PreviewWorkerMessage<T> {
        let mut state = self.lock_state();
        loop {
            if state.shutdown {
                return PreviewWorkerMessage::Shutdown;
            }
            // A graphics-context change must be applied before a pending frame.
            if let Some((handle, hwnd)) = state.latest_sharing_context.take() {
                return PreviewWorkerMessage::SetSharingContext(handle, hwnd);
            }
            if let Some(render) = state.latest_render.take() {
                return PreviewWorkerMessage::Render(render);
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    pub(super) fn shutdown(&self) {
        let mut state = self.lock_state();
        state.shutdown = true;
        state.latest_render = None;
        state.latest_sharing_context = None;
        drop(state);
        self.ready.notify_all();
    }

    fn lock_state(&self) -> MutexGuard<'_, PreviewMailboxState<T>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn pending_render_count(&self) -> usize {
        usize::from(self.lock_state().latest_render.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::{PreviewMailbox, PreviewSubmission, PreviewWorkerMessage};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn pending_preview_is_bounded_and_keeps_only_the_latest_value() {
        let inbox = PreviewMailbox::new();
        assert_eq!(inbox.submit_render(0_u64), PreviewSubmission::Accepted);
        for value in 1..10_000 {
            assert_eq!(
                inbox.submit_render(value),
                PreviewSubmission::ReplacedPending
            );
        }

        assert_eq!(inbox.pending_render_count(), 1);
        let PreviewWorkerMessage::Render(value) = inbox.recv() else {
            panic!("a render value must be ready");
        };
        assert_eq!(value, 9_999);
        assert_eq!(inbox.pending_render_count(), 0);
    }

    #[test]
    fn sharing_context_is_coalesced_and_applied_before_render() {
        let inbox = PreviewMailbox::new();
        assert!(inbox.set_sharing_context(1, Some(2)));
        assert!(inbox.set_sharing_context(3, Some(4)));
        assert_eq!(inbox.submit_render(5_u64), PreviewSubmission::Accepted);

        let PreviewWorkerMessage::SetSharingContext(handle, hwnd) = inbox.recv() else {
            panic!("sharing context must precede the pending frame");
        };
        assert_eq!((handle, hwnd), (3, Some(4)));
        let PreviewWorkerMessage::Render(value) = inbox.recv() else {
            panic!("render must remain queued after the context update");
        };
        assert_eq!(value, 5);
    }

    #[test]
    fn worker_can_replace_a_dequeued_stale_frame_before_evaluation() {
        let inbox = PreviewMailbox::new();
        assert_eq!(inbox.submit_render(10_u64), PreviewSubmission::Accepted);
        let PreviewWorkerMessage::Render(first) = inbox.recv() else {
            panic!("first render must be ready");
        };
        assert_eq!(inbox.submit_render(11_u64), PreviewSubmission::Accepted);

        assert_eq!(inbox.take_newer_render().unwrap_or(first), 11);
        assert_eq!(inbox.pending_render_count(), 0);
    }

    #[test]
    fn context_change_is_not_skipped_when_a_newer_frame_arrives() {
        let inbox = PreviewMailbox::new();
        assert_eq!(inbox.submit_render(10_u64), PreviewSubmission::Accepted);
        let PreviewWorkerMessage::Render(first) = inbox.recv() else {
            panic!("first render must be ready");
        };
        assert!(inbox.set_sharing_context(20, Some(30)));
        assert_eq!(inbox.submit_render(11_u64), PreviewSubmission::Accepted);

        assert_eq!(inbox.take_newer_render(), None);
        assert_eq!(first, 10);
        assert!(matches!(
            inbox.recv(),
            PreviewWorkerMessage::SetSharingContext(20, Some(30))
        ));
        assert!(matches!(inbox.recv(), PreviewWorkerMessage::Render(11)));
    }

    #[test]
    fn shutdown_unblocks_an_idle_worker_and_closes_submission() {
        let inbox = Arc::new(PreviewMailbox::<u64>::new());
        let worker_inbox = Arc::clone(&inbox);
        let worker = thread::spawn(move || worker_inbox.recv());
        inbox.shutdown();

        assert!(matches!(
            worker.join().unwrap(),
            PreviewWorkerMessage::Shutdown
        ));
        assert_eq!(inbox.submit_render(1), PreviewSubmission::Closed);
        assert!(!inbox.set_sharing_context(1, None));
    }
}
