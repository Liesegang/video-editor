use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::plugin::{ExportFormat, ExportSettings, PluginManager};
use crate::rendering::renderer::Renderer;
use crate::service::project_model::ProjectModel;
use crate::service::render_service::RenderService;
use crate::util::timing::{ScopedTimer, measure_info};
use log::{error, info};
use std::ops::Range;
use std::sync::Arc;
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};

struct SaveTask {
    frame_index: u64,
    output_path: String,
    image: Image,
    export_settings: Arc<ExportSettings>,
}

pub struct ExportService {
    save_tx: Option<SyncSender<SaveTask>>,
    saver_handle: Option<JoinHandle<()>>,
    export_settings: Arc<ExportSettings>,
}

impl ExportService {
    pub fn new(
        plugin_manager: Arc<PluginManager>,
        export_settings: Arc<ExportSettings>,
        save_queue_bound: usize,
    ) -> Self {
        let queue_bound = save_queue_bound.max(1);
        let (save_tx, save_rx) = mpsc::sync_channel::<SaveTask>(queue_bound);
        let saver_handle = thread::spawn(move || {
            while let Ok(task) = save_rx.recv() {
                if let Err(err) = plugin_manager.export_image(
                    "png_export", // Hardcoded exporter_id
                    &task.output_path,
                    &task.image,
                    &task.export_settings,
                ) {
                    error!(
                        "Failed to save frame {} to {}: {}",
                        task.frame_index, task.output_path, err
                    );
                    break;
                }
            }
        });

        Self {
            save_tx: Some(save_tx),
            saver_handle: Some(saver_handle),
            export_settings,
        }
    }

    pub fn render_range<T: Renderer>(
        &mut self,
        render_service: &mut RenderService<T>,
        project_model: &ProjectModel,
        frame_range: Range<u64>,
        output_stem: &str,
    ) -> Result<(), LibraryError> {
        let composition = project_model.composition();
        let (fps, total_frames) = (composition.fps, composition.duration.ceil().max(0.0) as u64);
        let sender = self.save_tx.as_ref().ok_or(LibraryError::Render(
            "Save queue is already closed".to_string(),
        ))?;
        let export_format = self.export_settings.export_format();
        let video_output = if matches!(export_format, ExportFormat::Video) {
            Some(format!(
                "{}.{}",
                output_stem, self.export_settings.container
            ))
        } else {
            None
        };

        for frame_index in frame_range {
            if frame_index >= total_frames {
                break;
            }

            info!("Render frame {}:", frame_index);
            let _frame_scope = ScopedTimer::info(format!("Frame {} total", frame_index));

            let frame_time = frame_index as f64 / fps;
            let image = measure_info(format!("Frame {}: renderer pass", frame_index), || {
                render_service.render_frame(project_model, frame_time)
            })?;

            let output_path = match export_format {
                ExportFormat::Png => format!("{}_{:03}.png", output_stem, frame_index),
                ExportFormat::Video => video_output.clone().unwrap_or_else(|| {
                    format!("{}.{}", output_stem, self.export_settings.container)
                }),
            };
            sender
                .send(SaveTask {
                    frame_index,
                    output_path,
                    image,
                    export_settings: Arc::clone(&self.export_settings),
                })
                .map_err(|_| LibraryError::Render("Save queue disconnected".to_string()))?;
        }

        Ok(())
    }

    pub fn shutdown(mut self) -> Result<(), LibraryError> {
        self.save_tx.take();
        if let Some(handle) = self.saver_handle.take() {
            handle
                .join()
                .map_err(|_| LibraryError::Render("Failed to join save worker".to_string()))?;
        }
        Ok(())
    }
}

impl Drop for ExportService {
    fn drop(&mut self) {
        self.save_tx.take();
        if let Some(handle) = self.saver_handle.take() {
            let _ = handle.join();
        }
    }
}
