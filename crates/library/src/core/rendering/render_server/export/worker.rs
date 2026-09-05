use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use crate::cache::SharedCacheManager;
use crate::plugin::PluginManager;

use super::super::AuthoringExportResult;
use super::panic_guard::{ExportFailureContext, catch_export_panic};
use super::{
    AuthoringExportRenderer, AuthoringExportRequest, run_authoring_png_export,
    run_authoring_video_export,
};

pub(in crate::core::rendering::render_server) fn run_authoring_export_worker(
    receiver: Receiver<AuthoringExportRequest>,
    result_sender: Sender<AuthoringExportResult>,
    plugin_manager: Arc<PluginManager>,
    cache_manager: SharedCacheManager,
) {
    let mut renderer: Option<AuthoringExportRenderer> = None;
    while let Ok(request) = receiver.recv() {
        let (failure_context, (result, worker_panicked)) = match request {
            AuthoringExportRequest::Png(request) => {
                let context = ExportFailureContext::capture_png(&request);
                let guarded = catch_export_panic("authoring PNG export worker request", || {
                    Ok(run_authoring_png_export(
                        request,
                        &mut renderer,
                        &plugin_manager,
                        &cache_manager,
                    ))
                });
                (context, guarded)
            }
            AuthoringExportRequest::Video(request) => {
                let context = ExportFailureContext::capture_video(&request);
                let guarded = catch_export_panic("authoring video export worker request", || {
                    Ok(run_authoring_video_export(
                        request,
                        &mut renderer,
                        &plugin_manager,
                        &cache_manager,
                    ))
                });
                (context, guarded)
            }
            AuthoringExportRequest::Shutdown => break,
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if worker_panicked {
                    renderer = None;
                }
                failure_context.into_result(error)
            }
        };
        if result_sender.send(result).is_err() {
            break;
        }
    }
}
