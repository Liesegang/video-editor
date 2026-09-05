use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::error::LibraryError;
use crate::model::authoring::TimelineId;
use crate::model::frame::frame::FrameInfo;

use super::super::{AuthoringExportResult, RenderRequestId, authoring_error_frame_info};
use super::{AuthoringPngExportRequest, AuthoringVideoExportRequest, authoring_video_frame_count};

pub(super) struct ExportFailureContext {
    request_id: RenderRequestId,
    timeline_id: TimelineId,
    frame_number: i64,
    output_path: String,
    frame_info: FrameInfo,
    frame_count: u64,
}

impl ExportFailureContext {
    pub(super) fn capture_png(request: &AuthoringPngExportRequest) -> Self {
        Self {
            request_id: request.request_id,
            timeline_id: request.timeline_id,
            frame_number: request.frame_number,
            output_path: request.output_path.clone(),
            frame_info: authoring_error_frame_info(
                request.project.as_ref(),
                request.timeline_id,
                request.frame_number,
                1.0,
                None,
            ),
            frame_count: 1,
        }
    }

    pub(super) fn capture_video(request: &AuthoringVideoExportRequest) -> Self {
        Self {
            request_id: request.request_id,
            timeline_id: request.timeline_id,
            frame_number: 0,
            output_path: request.output_path.clone(),
            frame_info: authoring_error_frame_info(
                request.project.as_ref(),
                request.timeline_id,
                0,
                1.0,
                None,
            ),
            frame_count: authoring_video_frame_count(request.project.as_ref(), request.timeline_id)
                .unwrap_or(0),
        }
    }

    pub(super) fn into_result(self, error: LibraryError) -> AuthoringExportResult {
        AuthoringExportResult {
            request_id: self.request_id,
            timeline_id: self.timeline_id,
            frame_number: self.frame_number,
            output_path: self.output_path,
            output: Err(error),
            frame_info: self.frame_info,
            frames_exported: 0,
            published: false,
            frame_count: self.frame_count,
        }
    }
}

pub(super) fn catch_export_panic<T>(
    scope: &str,
    operation: impl FnOnce() -> Result<T, LibraryError>,
) -> (Result<T, LibraryError>, bool) {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => (result, false),
        Err(payload) => (
            Err(LibraryError::Runtime(format!(
                "{scope} panicked: {}",
                panic_payload(payload.as_ref())
            ))),
            true,
        ),
    }
}

fn panic_payload(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}
