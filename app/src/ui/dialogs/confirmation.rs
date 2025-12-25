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
