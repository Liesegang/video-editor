use eframe::egui;
use library::{AuthoringExportResult, LibraryError, RenderRequestId};

use crate::state::authoring::AuthoringUiState;

use super::startup::qa_export_path;
use super::RuViEApp;

#[derive(Default)]
pub(super) struct AuthoringExportRuntime {
    pending: Option<(RenderRequestId, String)>,
    next_request: u64,
    reported_error: Option<String>,
}

impl AuthoringExportRuntime {
    pub(super) fn for_app() -> Self {
        Self {
            pending: None,
            next_request: 1_u64 << 62,
            reported_error: None,
        }
    }

    fn clear_reported_error(&mut self, state: &mut AuthoringUiState) {
        if self
            .reported_error
            .as_ref()
            .is_some_and(|reported| state.error.as_ref() == Some(reported))
        {
            state.error = None;
        }
        self.reported_error = None;
    }

    fn report_error(&mut self, state: &mut AuthoringUiState, error: String) {
        state.error = Some(error.clone());
        self.reported_error = Some(error);
    }

    fn begin(
        &mut self,
        state: &mut AuthoringUiState,
        request_id: RenderRequestId,
        output_path: String,
    ) {
        self.clear_reported_error(state);
        state.status = format!("Exporting {output_path}");
        self.pending = Some((request_id, output_path));
    }

    fn apply_completion(
        &mut self,
        state: &mut AuthoringUiState,
        result: AuthoringExportResult,
    ) -> bool {
        let expected = self
            .pending
            .as_ref()
            .is_some_and(|(request_id, _)| *request_id == result.request_id);
        if !expected {
            return false;
        }

        self.pending = None;
        match result.output {
            Ok(()) if result.published => {
                self.clear_reported_error(state);
                state.status = format!(
                    "Exported {} frames to {}",
                    result.frames_exported, result.output_path
                );
            }
            Ok(()) => {
                state.status = format!("Export failed for {}", result.output_path);
                self.report_error(
                    state,
                    format!(
                        "Export failed for {}: the worker completed without publishing output",
                        result.output_path
                    ),
                );
            }
            Err(error) => {
                state.status = format!("Export failed for {}", result.output_path);
                self.report_error(
                    state,
                    format!("Export failed for {}: {error}", result.output_path),
                );
            }
        }
        true
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
        self.export_runtime
            .begin(&mut self.state, request_id, output_path);
        Ok(())
    }

    pub(super) fn poll_export(&mut self, context: &egui::Context) {
        while let Ok(result) = self.render_server.poll_authoring_export_result() {
            self.export_runtime
                .apply_completion(&mut self.state, result);
        }
        if self.export_runtime.pending.is_some() {
            context.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use library::model::authoring::TimelineId;
    use library::model::frame::color::Color;
    use library::model::frame::frame::FrameInfo;

    use super::*;

    fn completion(
        request_id: RenderRequestId,
        output_path: &str,
        output: Result<(), LibraryError>,
        published: bool,
    ) -> AuthoringExportResult {
        AuthoringExportResult {
            request_id,
            timeline_id: TimelineId::new(),
            frame_number: 0,
            output_path: output_path.to_string(),
            output,
            frame_info: FrameInfo {
                width: 1,
                height: 1,
                background_color: Color::black(),
                color_profile: "sRGB".to_string(),
                render_scale: OrderedFloat(1.0),
                now_time: OrderedFloat(0.0),
                region: None,
                items: Vec::new(),
            },
            frames_exported: 0,
            published,
            frame_count: 1,
        }
    }

    fn exporting_runtime(
        request_id: RenderRequestId,
        output_path: &str,
    ) -> (AuthoringExportRuntime, AuthoringUiState) {
        let mut runtime = AuthoringExportRuntime::for_app();
        let mut state = AuthoringUiState::new(TimelineId::new());
        runtime.begin(&mut state, request_id, output_path.to_string());
        (runtime, state)
    }

    #[test]
    fn failed_export_completion_clears_pending_status_and_surfaces_error() {
        let request_id = RenderRequestId::new(10);
        let output_path = "C:/output/failed.mp4";
        let (mut runtime, mut state) = exporting_runtime(request_id, output_path);

        assert!(runtime.apply_completion(
            &mut state,
            completion(
                request_id,
                output_path,
                Err(LibraryError::Render("encoder stopped".to_string())),
                false,
            ),
        ));

        assert!(runtime.pending.is_none());
        assert_eq!(runtime.reported_error, state.error);
        assert_eq!(state.status, format!("Export failed for {output_path}"));
        assert_eq!(
            state.error.as_deref(),
            Some("Export failed for C:/output/failed.mp4: Rendering error: encoder stopped")
        );
    }

    #[test]
    fn unpublished_export_completion_clears_pending_status_and_surfaces_error() {
        let request_id = RenderRequestId::new(11);
        let output_path = "C:/output/unpublished.mp4";
        let (mut runtime, mut state) = exporting_runtime(request_id, output_path);

        assert!(runtime.apply_completion(
            &mut state,
            completion(request_id, output_path, Ok(()), false),
        ));

        assert!(runtime.pending.is_none());
        assert_eq!(runtime.reported_error, state.error);
        assert_eq!(state.status, format!("Export failed for {output_path}"));
        assert_eq!(
            state.error.as_deref(),
            Some(
                "Export failed for C:/output/unpublished.mp4: the worker completed without publishing output"
            )
        );
    }

    #[test]
    fn unrelated_export_completion_does_not_clear_active_progress() {
        let active_request = RenderRequestId::new(12);
        let output_path = "C:/output/active.mp4";
        let (mut runtime, mut state) = exporting_runtime(active_request, output_path);

        assert!(!runtime.apply_completion(
            &mut state,
            completion(
                RenderRequestId::new(13),
                "C:/output/stale.mp4",
                Err(LibraryError::Render("stale failure".to_string())),
                false,
            ),
        ));

        assert_eq!(
            runtime.pending,
            Some((active_request, output_path.to_string()))
        );
        assert_eq!(state.status, format!("Exporting {output_path}"));
        assert!(state.error.is_none());
    }

    #[test]
    fn retry_after_failed_export_clears_only_the_previous_export_error() {
        let failed_request = RenderRequestId::new(14);
        let failed_path = "C:/output/failed.mp4";
        let (mut runtime, mut state) = exporting_runtime(failed_request, failed_path);
        assert!(runtime.apply_completion(
            &mut state,
            completion(
                failed_request,
                failed_path,
                Err(LibraryError::Render("encoder stopped".to_string())),
                false,
            ),
        ));

        let retry_request = RenderRequestId::new(15);
        let retry_path = "C:/output/retry.mp4";
        runtime.begin(&mut state, retry_request, retry_path.to_string());

        assert_eq!(
            runtime.pending,
            Some((retry_request, retry_path.to_string()))
        );
        assert_eq!(state.status, format!("Exporting {retry_path}"));
        assert!(state.error.is_none());
        assert!(runtime.reported_error.is_none());

        runtime.reported_error = Some("old export error".to_string());
        state.error = Some("Audio device failed".to_string());
        runtime.begin(&mut state, RenderRequestId::new(16), retry_path.to_string());
        assert_eq!(state.error.as_deref(), Some("Audio device failed"));
        assert!(runtime.reported_error.is_none());
    }
}
