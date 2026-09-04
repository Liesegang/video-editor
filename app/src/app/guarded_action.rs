//! Single-flight state machine for destructive project actions.
//!
//! A dialog decision and the destructive action deliberately happen on
//! different UI frames. This lets egui publish the dismissed dialog before a
//! native file picker, project replacement, or viewport close begins.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuardedProjectAction {
    NewProject,
    OpenProject,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    Requested(GuardedProjectAction),
    AwaitingChoice(GuardedProjectAction),
    Ready(GuardedProjectAction),
    CloseCommitted,
}

#[derive(Debug, Default)]
pub(crate) struct GuardedActionState {
    phase: Phase,
}

impl GuardedActionState {
    /// Queues one action. Requests arriving while another action is in flight
    /// are ignored so one user decision can never execute multiple actions.
    pub(crate) fn request(&mut self, action: GuardedProjectAction) -> bool {
        if self.phase != Phase::Idle {
            return false;
        }
        self.phase = Phase::Requested(action);
        true
    }

    /// Resolves the dirty check for a newly requested action, without
    /// executing it in the request's input frame.
    pub(crate) fn resolve_request(&mut self, has_unsaved_changes: bool) -> bool {
        let Phase::Requested(action) = self.phase else {
            return false;
        };
        self.phase = if has_unsaved_changes {
            Phase::AwaitingChoice(action)
        } else {
            Phase::Ready(action)
        };
        true
    }

    pub(crate) fn prompt_action(&self) -> Option<GuardedProjectAction> {
        match self.phase {
            Phase::AwaitingChoice(action) => Some(action),
            _ => None,
        }
    }

    /// Applies a modal decision. `Save` keeps the prompt active until the
    /// caller confirms that saving completed; cancelling a native Save As
    /// dialog therefore cannot accidentally discard the project.
    pub(crate) fn choose(&mut self, choice: UnsavedChoice) -> bool {
        let Phase::AwaitingChoice(action) = self.phase else {
            return false;
        };
        match choice {
            UnsavedChoice::Save => return true,
            UnsavedChoice::Discard => self.phase = Phase::Ready(action),
            UnsavedChoice::Cancel => self.phase = Phase::Idle,
        }
        false
    }

    pub(crate) fn finish_save(&mut self, saved: bool) {
        if !saved {
            return;
        }
        if let Phase::AwaitingChoice(action) = self.phase {
            self.phase = Phase::Ready(action);
        }
    }

    /// Takes an approved action exactly once. Quit commits close handling
    /// before the viewport command is emitted, preventing a second native
    /// close request from reopening the prompt.
    pub(crate) fn take_ready_action(&mut self) -> Option<GuardedProjectAction> {
        let Phase::Ready(action) = self.phase else {
            return None;
        };
        self.phase = if action == GuardedProjectAction::Quit {
            Phase::CloseCommitted
        } else {
            Phase::Idle
        };
        Some(action)
    }

    pub(crate) fn blocks_commands(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub(crate) fn allows_window_close(&self) -> bool {
        self.phase == Phase::CloseCommitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discard_is_deferred_and_executes_exactly_once_for_every_action() {
        for action in [
            GuardedProjectAction::NewProject,
            GuardedProjectAction::OpenProject,
            GuardedProjectAction::Quit,
        ] {
            let mut state = GuardedActionState::default();
            assert!(state.request(action));
            assert!(state.resolve_request(true));
            assert_eq!(state.prompt_action(), Some(action));
            assert!(!state.choose(UnsavedChoice::Discard));
            assert_eq!(state.take_ready_action(), Some(action));
            assert_eq!(state.take_ready_action(), None);
        }
    }

    #[test]
    fn cancel_returns_to_idle_without_executing() {
        let mut state = GuardedActionState::default();
        assert!(state.request(GuardedProjectAction::OpenProject));
        assert!(state.resolve_request(true));
        assert!(!state.choose(UnsavedChoice::Cancel));
        assert_eq!(state.take_ready_action(), None);
        assert!(!state.blocks_commands());
    }

    #[test]
    fn save_requires_confirmation_before_the_action_is_ready() {
        let mut state = GuardedActionState::default();
        assert!(state.request(GuardedProjectAction::NewProject));
        assert!(state.resolve_request(true));
        assert!(state.choose(UnsavedChoice::Save));
        assert_eq!(state.take_ready_action(), None);

        state.finish_save(false);
        assert_eq!(
            state.prompt_action(),
            Some(GuardedProjectAction::NewProject)
        );
        state.finish_save(true);
        assert_eq!(
            state.take_ready_action(),
            Some(GuardedProjectAction::NewProject)
        );
        assert_eq!(state.take_ready_action(), None);
    }

    #[test]
    fn one_in_flight_action_rejects_competing_requests() {
        let mut state = GuardedActionState::default();
        assert!(state.request(GuardedProjectAction::NewProject));
        assert!(!state.request(GuardedProjectAction::OpenProject));
        assert!(!state.request(GuardedProjectAction::Quit));
        assert!(state.resolve_request(true));
        assert!(!state.request(GuardedProjectAction::OpenProject));
        assert_eq!(
            state.prompt_action(),
            Some(GuardedProjectAction::NewProject)
        );
    }

    #[test]
    fn clean_action_still_waits_for_the_next_frame() {
        let mut state = GuardedActionState::default();
        assert!(state.request(GuardedProjectAction::OpenProject));
        assert_eq!(state.take_ready_action(), None);
        assert!(state.resolve_request(false));
        assert_eq!(
            state.take_ready_action(),
            Some(GuardedProjectAction::OpenProject)
        );
    }

    #[test]
    fn committed_quit_allows_only_the_followup_window_close() {
        let mut state = GuardedActionState::default();
        assert!(state.request(GuardedProjectAction::Quit));
        assert!(state.resolve_request(false));
        assert!(!state.allows_window_close());
        assert_eq!(state.take_ready_action(), Some(GuardedProjectAction::Quit));
        assert!(state.allows_window_close());
        assert!(!state.request(GuardedProjectAction::Quit));
        assert_eq!(state.take_ready_action(), None);
    }
}
