use crate::error::LibraryError;
use crate::io::image::Image;
use crate::extensions::traits::{ExportFormat, ExportSettings, PluginManager};
use crate::graphics::renderer::Renderer;
use crate::app::project_model::ProjectModel;
use crate::compositing::render_service::RenderService;
use crate::util::timing::{ScopedTimer, measure_info};
use log::{error, info};

use std::ops::Range;
use std::sync::Arc;
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};

struct SaveTask {
    exporter_id: String,
    frame_index: u64,
    output_path: String,
    image: Image,
    export_settings: Arc<ExportSettings>,
}

pub struct ExportService {
    save_tx: Option<SyncSender<SaveTask>>,
    saver_handle: Option<JoinHandle<()>>,
    export_settings: Arc<ExportSettings>,
    exporter_id: String,
    temp_files: Vec<String>,
}

impl ExportService {
    pub fn new(
        plugin_manager: Arc<PluginManager>,
        exporter_id: String,
        export_settings: Arc<ExportSettings>,
        save_queue_bound: usize,
    ) -> Self {
        let queue_bound = save_queue_bound.max(1);
        let (save_tx, save_rx) = mpsc::sync_channel::<SaveTask>(queue_bound);
        let saver_handle = thread::spawn(move || {
            while let Ok(task) = save_rx.recv() {
                if let Err(err) = plugin_manager.export_image(
                    &task.exporter_id,
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
            exporter_id,
            temp_files: Vec::new(),
            // ...
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
        let (fps, total_frames) = (
            composition.fps,
            (composition.duration * composition.fps).ceil().max(0.0) as u64,
        );
        let sender = self.save_tx.as_ref().ok_or(LibraryError::Render(
            "Save queue is already closed".to_string(),
        ))?;

        // Prepare Settings (Potentially with Audio)
        let settings_struct = (*self.export_settings).clone();
        let export_format = settings_struct.export_format();

        // Setup Output Paths
        let mut base_template = output_stem.replace("{project}", &project_model.project().name);
        base_template = base_template.replace("{composition}", &composition.name);
        let has_frame_token = base_template.contains("{frame");

        let video_output = if matches!(export_format, ExportFormat::Video) {
            // Audio is now handled by the caller (ExportDialog/lib.rs) who pre-renders it
            // and adds "audio_source" to ExportSettings.

            // Render Video
            let clean_stem = if has_frame_token {
                Self::format_frame_token_in_string(&base_template, frame_range.start)
            } else {
                base_template.clone()
            };

            Some(format!("{}.{}", clean_stem, settings_struct.container))
        } else {
            None
        };

        let settings_arc = Arc::new(settings_struct);

        for frame_index in frame_range {
            if frame_index >= total_frames {
                break;
            }

            info!("Render frame {}:", frame_index);
            let _frame_scope = ScopedTimer::info(format!("Frame {} total", frame_index));

            let frame_time = frame_index as f64 / fps;
            let output = measure_info(format!("Frame {}: renderer pass", frame_index), || {
                render_service.render_frame(project_model, frame_time)
            })?;

            let image = match output {
                crate::graphics::renderer::RenderOutput::Image(img) => img,
                crate::graphics::renderer::RenderOutput::Texture(_) => {
                    return Err(LibraryError::Render(
                        "Export received Texture output (unsupported)".to_string(),
                    ));
                }
            };

            let output_path = match export_format {
                ExportFormat::Png => {
                    if has_frame_token {
                        let name = Self::format_frame_token_in_string(&base_template, frame_index);
                        format!("{}.png", name)
                    } else {
                        format!("{}_{:03}.png", base_template, frame_index)
                    }
                }
                ExportFormat::Video => video_output.clone().unwrap_or_else(|| {
                    format!("{}.{}", base_template, self.export_settings.container)
                }),
            };
            sender
                .send(SaveTask {
                    exporter_id: self.exporter_id.clone(),
                    frame_index,
                    output_path,
                    image,
                    export_settings: Arc::clone(&settings_arc), // Use the modified settings
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
    fn format_frame_token_in_string(path: &str, frame: u64) -> String {
        let mut result = String::new();
        let mut chars = path.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '{' {
                // Potential token start
                let mut token_buffer = String::new();
                let mut is_token = false;

                // Clone the iterator to check ahead without consuming if it's not a token
                let mut check_chars = chars.clone();
                while let Some(tc) = check_chars.next() {
                    if tc == '}' {
                        is_token = true;
                        break;
                    }
                    token_buffer.push(tc);
                }

                if is_token {
                    if token_buffer == "frame" {
                        result.push_str(&frame.to_string());
                        // Advance main iterator past the token
                        for _ in 0..token_buffer.len() + 1 {
                            chars.next();
                        }
                        continue;
                    } else if token_buffer.starts_with("frame:") {
                        let spec = &token_buffer["frame:".len()..];
                        // Parse "0N" or just "N"
                        if let Ok(width) = spec.parse::<usize>() {
                            result.push_str(&format!("{:0width$}", frame, width = width));
                            for _ in 0..token_buffer.len() + 1 {
                                chars.next();
                            }
                            continue;
                        }
                    }
                }
            }
            result.push(c);
        }
        result
    }
}

impl Drop for ExportService {
    fn drop(&mut self) {
        self.save_tx.take();
        if let Some(handle) = self.saver_handle.take() {
            let _ = handle.join();
        }
        for path in &self.temp_files {
            if let Err(e) = std::fs::remove_file(path) {
                error!("Failed to remove temp file {}: {}", path, e);
            }
        }
    }
}
