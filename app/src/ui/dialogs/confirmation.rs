use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ConfirmationAction {
    DeleteComposition(Uuid),
    DeleteAsset(Uuid),
    RemoveTrack {
        composition_id: Uuid,
        track_id: Uuid,
    },
    // Add other actions as needed
}

#[derive(Clone, Debug)]
pub struct ConfirmationDialog {
    pub is_open: bool,
    pub title: String,
    pub message: String,
    pub action: Option<ConfirmationAction>,
}

impl Default for ConfirmationDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            title: "Confirmation".to_string(),
            message: "Are you sure?".to_string(),
            action: None,
        }
    }
}

impl ConfirmationDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        action: ConfirmationAction,
    ) {
        self.title = title.into();
        self.message = message.into();
        self.action = Some(action);
        self.is_open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<ConfirmationAction> {
        let mut confirmed_action = None;
        let mut should_close = false;

        if self.is_open {
            // Use our existing modal widget if possible, or standard egui window
            // Assuming we want to stick to a simple window for this refactor to avoid dependency chains
            let mut open = true;
            egui::Window::new(&self.title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(&self.message);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            should_close = true;
                        }
                        if ui
                            .button(egui::RichText::new("Confirm").color(egui::Color32::RED))
                            .clicked()
                        {
                            confirmed_action = self.action.clone();
                            should_close = true;
                        }
                    });
                });

            if !open {
                should_close = true;
            }
        }

        if should_close {
            self.is_open = false;
        }

        confirmed_action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    // ── Domain: Dialog State Management ──

    #[test]
    fn default_dialog_is_closed() {
        let dialog = ConfirmationDialog::default();
        assert!(!dialog.is_open);
        assert!(dialog.action.is_none());
        assert_eq!(dialog.title, "Confirmation");
        assert_eq!(dialog.message, "Are you sure?");
    }

    #[test]
    fn open_sets_title_message_and_action() {
        let mut dialog = ConfirmationDialog::new();
        let id = Uuid::new_v4();
        dialog.open(
            "Delete?",
            "This cannot be undone.",
            ConfirmationAction::DeleteAsset(id),
        );

        assert!(dialog.is_open);
        assert_eq!(dialog.title, "Delete?");
        assert_eq!(dialog.message, "This cannot be undone.");
        assert_eq!(dialog.action, Some(ConfirmationAction::DeleteAsset(id)));
    }

    // ── Domain: Dialog UI Rendering ──

    #[test]
    fn shows_message_when_open() {
        let mut dialog = ConfirmationDialog::default();
        dialog.open(
            "Confirm Delete",
            "This action cannot be undone.",
            ConfirmationAction::DeleteAsset(Uuid::new_v4()),
        );
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                dialog.show(ctx);
            });
        assert!(harness
            .query_by_label("This action cannot be undone.")
            .is_some());
    }

    #[test]
    fn shows_cancel_and_confirm_buttons() {
        let mut dialog = ConfirmationDialog::default();
        dialog.open(
            "Delete Composition",
            "Proceed?",
            ConfirmationAction::DeleteComposition(Uuid::new_v4()),
        );
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                dialog.show(ctx);
            });
        assert!(harness.query_by_label("Cancel").is_some());
        assert!(harness.query_by_label("Confirm").is_some());
    }

    #[test]
    fn hidden_when_not_open() {
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(|ctx| {
                let mut d = ConfirmationDialog::default();
                d.show(ctx);
            });
        assert!(harness.query_by_label("Are you sure?").is_none());
    }

    #[test]
    fn shows_custom_title() {
        let mut dialog = ConfirmationDialog::default();
        dialog.open(
            "Remove Track",
            "Remove this track and all its clips?",
            ConfirmationAction::RemoveTrack {
                composition_id: Uuid::new_v4(),
                track_id: Uuid::new_v4(),
            },
        );
        let harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                dialog.show(ctx);
            });
        assert!(harness.query_by_label("Remove Track").is_some());
        assert!(harness
            .query_by_label("Remove this track and all its clips?")
            .is_some());
    }

    // ── Domain: Dialog Interaction (Dynamic / Selenium-style) ──

    #[test]
    fn click_confirm_returns_action() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let action_id = Uuid::new_v4();
        let dialog = Rc::new(RefCell::new({
            let mut d = ConfirmationDialog::default();
            d.open(
                "Delete Item",
                "Sure?",
                ConfirmationAction::DeleteAsset(action_id),
            );
            d
        }));
        let results: Rc<RefCell<Vec<Option<ConfirmationAction>>>> =
            Rc::new(RefCell::new(Vec::new()));

        let d = dialog.clone();
        let r = results.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                let result = d.borrow_mut().show(ctx);
                r.borrow_mut().push(result);
            });

        harness.get_by_label("Confirm").click();
        harness.run();

        // run() processes multiple frames until convergence; the action is
        // returned in the click-processing frame, then None on subsequent frames
        // (dialog already closed). So we look for the action across all frames.
        let results = results.borrow();
        let confirmed: Vec<_> = results.iter().filter_map(|r| r.clone()).collect();
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0], ConfirmationAction::DeleteAsset(action_id));
    }

    #[test]
    fn click_cancel_returns_no_action() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let dialog = Rc::new(RefCell::new({
            let mut d = ConfirmationDialog::default();
            d.open(
                "Remove Asset",
                "Really remove?",
                ConfirmationAction::DeleteAsset(Uuid::new_v4()),
            );
            d
        }));
        let results: Rc<RefCell<Vec<Option<ConfirmationAction>>>> =
            Rc::new(RefCell::new(Vec::new()));

        let d = dialog.clone();
        let r = results.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                let result = d.borrow_mut().show(ctx);
                r.borrow_mut().push(result);
            });

        harness.get_by_label("Cancel").click();
        harness.run();

        // Cancel should never return an action in any frame
        let results = results.borrow();
        let confirmed: Vec<_> = results.iter().filter_map(|r| r.clone()).collect();
        assert!(confirmed.is_empty());
    }

    #[test]
    fn click_confirm_closes_dialog() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let dialog = Rc::new(RefCell::new({
            let mut d = ConfirmationDialog::default();
            d.open(
                "Discard Changes",
                "Unsaved changes will be lost.",
                ConfirmationAction::DeleteComposition(Uuid::new_v4()),
            );
            d
        }));

        let d = dialog.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                d.borrow_mut().show(ctx);
            });

        assert!(dialog.borrow().is_open);

        harness.get_by_label("Confirm").click();
        harness.run();

        assert!(!dialog.borrow().is_open);
    }

    #[test]
    fn click_cancel_closes_dialog() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let dialog = Rc::new(RefCell::new({
            let mut d = ConfirmationDialog::default();
            d.open(
                "Remove Track",
                "This will remove the track.",
                ConfirmationAction::RemoveTrack {
                    composition_id: Uuid::new_v4(),
                    track_id: Uuid::new_v4(),
                },
            );
            d
        }));

        let d = dialog.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                d.borrow_mut().show(ctx);
            });

        assert!(dialog.borrow().is_open);

        harness.get_by_label("Cancel").click();
        harness.run();

        assert!(!dialog.borrow().is_open);
    }

    #[test]
    fn confirm_returns_correct_remove_track_action() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let comp_id = Uuid::new_v4();
        let track_id = Uuid::new_v4();
        let dialog = Rc::new(RefCell::new({
            let mut d = ConfirmationDialog::default();
            d.open(
                "Remove Track",
                "Are you sure?",
                ConfirmationAction::RemoveTrack {
                    composition_id: comp_id,
                    track_id,
                },
            );
            d
        }));
        let results: Rc<RefCell<Vec<Option<ConfirmationAction>>>> =
            Rc::new(RefCell::new(Vec::new()));

        let d = dialog.clone();
        let r = results.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build(move |ctx| {
                let result = d.borrow_mut().show(ctx);
                r.borrow_mut().push(result);
            });

        harness.get_by_label("Confirm").click();
        harness.run();

        let results = results.borrow();
        let confirmed: Vec<_> = results.iter().filter_map(|r| r.clone()).collect();
        assert_eq!(confirmed.len(), 1);
        assert_eq!(
            confirmed[0],
            ConfirmationAction::RemoveTrack {
                composition_id: comp_id,
                track_id,
            }
        );
    }
}
