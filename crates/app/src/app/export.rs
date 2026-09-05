use eframe::egui;
use library::{LibraryError, RenderRequestId};

use super::startup::qa_export_path;
use super::RuViEApp;

#[derive(Default)]
pub(super) struct AuthoringExportRuntime {
    pending: Option<(RenderRequestId, String)>,
    next_request: u64,
}

impl AuthoringExportRuntime {
    pub(super) fn for_app() -> Self {
        Self {
            pending: None,
            next_request: 1_u64 << 62,
        }
    }
}

impl RuViEApp {
    pub(super) fn export_active_timeline_video(&mut self) -> Result<(), LibraryError> {
        if self.export_runtime.pending.is_some() {
            return Err(LibraryError::Validation(
                "An export is already in progress".to_string(),
            ));
        }
        let (_, project, plan) = self
            .preview_runtime
            .snapshot_and_plan(&self.service)
            .map_err(LibraryError::Render)?;
        let timeline = project
            .timelines
            .get(&self.state.active_timeline_id)
            .ok_or_else(|| {
                LibraryError::Validation("The active Timeline no longer exists".to_string())
            })?;
        let file_name = format!("{}.mp4", timeline.name.replace(['/', '\\'], "-"));
        let path = match qa_export_path()? {
            Some(path) => path,
            None => {
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("MP4 Video", &["mp4"])
                    .add_filter("Matroska Video", &["mkv"])
                    .set_file_name(file_name)
                    .save_file()
                else {
                    return Ok(());
                };
                path
            }
        };
        let output_path = path.to_str().map(str::to_owned).ok_or_else(|| {
            LibraryError::Validation("Export path is not valid Unicode".to_string())
        })?;
        let request_id = RenderRequestId::new(self.export_runtime.next_request);
        self.export_runtime.next_request = self.export_runtime.next_request.wrapping_add(1);
        if !self
            .render_server
            .send_authoring_video_export_request_at_instance(
                request_id,
                project,
                plan,
                self.state.active_timeline_id,
                self.state.active_instance_path.clone(),
                output_path.clone(),
            )
        {
            return Err(LibraryError::Runtime(
                "Export worker is busy; try again after the current export finishes".to_string(),
            ));
        }
        self.export_runtime.pending = Some((request_id, output_path.clone()));
        self.state.status = format!("Exporting {output_path}");
        Ok(())
    }

    pub(super) fn poll_export(&mut self, context: &egui::Context) {
        while let Ok(result) = self.render_server.poll_authoring_export_result() {
            let expected = self
                .export_runtime
                .pending
                .as_ref()
                .is_some_and(|(request_id, _)| *request_id == result.request_id);
            if !expected {
                continue;
            }
            self.export_runtime.pending = None;
            match result.output {
                Ok(()) if result.published => {
                    self.state.error = None;
                    self.state.status = format!(
                        "Exported {} frames to {}",
                        result.frames_exported, result.output_path
                    );
                }
                Ok(()) => {
                    self.state.error = Some(
                        "Export failed: the worker completed without publishing output".to_string(),
                    );
                }
                Err(error) => self.state.error = Some(format!("Export failed: {error}")),
            }
        }
        if self.export_runtime.pending.is_some() {
            context.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
