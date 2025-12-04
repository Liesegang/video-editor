use crate::framing::{PropertyEvaluatorRegistry, get_frame_from_project};
use crate::load_project;
use crate::loader::image::Image;
use crate::model::project::project::{Composition, Project};
use crate::plugin::{ExportFormat, ExportSettings, PluginManager, load_plugins};
use crate::rendering::RenderContext;
use crate::rendering::effects::EffectRegistry;
use crate::util::timing::{ScopedTimer, measure_debug, measure_info};
use log::{error, info};
use std::error::Error;
use std::fs;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};

struct SaveTask {
    frame_index: u64,
    output_path: String,
    image: Image,
    format: ExportFormat,
    export_settings: Arc<ExportSettings>,
}

type RendererBackend = crate::rendering::skia_renderer::SkiaRenderer;

pub struct ProjectService {
    project: Project,
    composition_index: usize,
    render_context: RenderContext<RendererBackend>,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
    export_settings: Arc<ExportSettings>,
    export_format: ExportFormat,
    save_tx: Option<SyncSender<SaveTask>>,
    saver_handle: Option<JoinHandle<()>>,
    plugin_manager: Arc<PluginManager>,
}

impl ProjectService {
    pub fn from_project_path(
        project_path: &str,
        composition_index: usize,
        save_queue_bound: usize,
    ) -> Result<Self, Box<dyn Error>> {
        let project = measure_info(format!("Load project {}", project_path), || {
            let json = fs::read_to_string(project_path)?;
            load_project(&json)
        })?;
        Self::from_project(project, composition_index, save_queue_bound)
    }

    pub fn from_project(
        project: Project,
        composition_index: usize,
        save_queue_bound: usize,
    ) -> Result<Self, Box<dyn Error>> {
        let composition = project
            .compositions
            .get(composition_index)
            .ok_or_else(|| format!("Invalid composition index {}", composition_index))?;

        let renderer = RendererBackend::new(
            composition.width as u32,
            composition.height as u32,
            composition.background_color.clone(),
        );
        let plugin_manager = load_plugins();
        let property_evaluators = Arc::new(plugin_manager.build_property_registry());
        let effect_registry = Arc::new(EffectRegistry::new_with_defaults());
        let export_settings = Arc::new(Self::build_export_settings_for_project(
            &project,
            composition,
        ));
        let export_format = export_settings.export_format();

        let queue_bound = save_queue_bound.max(1);
        let (save_tx, save_rx) = mpsc::sync_channel::<SaveTask>(queue_bound);
        let saver_plugins = Arc::clone(&plugin_manager);
        let saver_handle = thread::spawn(move || {
            while let Ok(task) = save_rx.recv() {
                if let Err(err) = saver_plugins.export_image(
                    task.format,
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

        let render_context = RenderContext::new(
            renderer,
            Arc::clone(&plugin_manager),
            Arc::clone(&property_evaluators),
            Arc::clone(&effect_registry),
        );

        Ok(Self {
            project,
            composition_index,
            render_context,
            property_evaluators,
            export_settings,
            export_format,
            save_tx: Some(save_tx),
            saver_handle: Some(saver_handle),
            plugin_manager,
        })
    }

    pub fn composition(&self) -> &Composition {
        &self.project.compositions[self.composition_index]
    }

    pub fn load_property_plugin<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn Error>> {
        self.plugin_manager
            .load_property_plugin_from_file(path.as_ref())?;
        self.refresh_property_registry();
        Ok(())
    }

    pub fn render_range(
        &mut self,
        frame_range: Range<u64>,
        output_stem: &str,
    ) -> Result<(), Box<dyn Error>> {
        let (fps, total_frames) = {
            let composition = self.composition();
            (composition.fps, composition.duration.ceil().max(0.0) as u64)
        };
        let sender = self
            .save_tx
            .as_ref()
            .ok_or("Save queue is already closed")?;
        let export_format = self.export_format;
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

            measure_debug(
                format!("Frame {}: clear render target", frame_index),
                || self.render_context.clear(),
            )?;

            let frame_time = frame_index as f64 / fps;
            let frame = measure_debug(
                format!("Frame {}: assemble frame graph", frame_index),
                || {
                    get_frame_from_project(
                        &self.project,
                        self.composition_index,
                        frame_time,
                        &self.property_evaluators,
                    )
                },
            );

            let image = measure_info(format!("Frame {}: renderer pass", frame_index), || {
                self.render_context.render_frame(frame)
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
                    format: export_format,
                    export_settings: Arc::clone(&self.export_settings),
                })
                .map_err(|_| "Save queue disconnected")?;
        }

        Ok(())
    }

    pub fn shutdown(mut self) -> Result<(), Box<dyn Error>> {
        self.save_tx.take();
        if let Some(handle) = self.saver_handle.take() {
            handle
                .join()
                .map_err(|_| -> Box<dyn Error> { "Failed to join save worker".into() })?;
        }
        Ok(())
    }
}

impl Drop for ProjectService {
    fn drop(&mut self) {
        self.save_tx.take();
        if let Some(handle) = self.saver_handle.take() {
            let _ = handle.join();
        }
    }
}

impl ProjectService {
    fn refresh_property_registry(&mut self) {
        let registry = Arc::new(self.plugin_manager.build_property_registry());
        self.property_evaluators = Arc::clone(&registry);
        self.render_context
            .set_property_evaluators(Arc::clone(&self.property_evaluators));
        self.rebuild_export_settings();
    }

    fn rebuild_export_settings(&mut self) {
        let composition = self.composition().clone();
        let settings = Arc::new(Self::build_export_settings_for_project(
            &self.project,
            &composition,
        ));
        self.export_format = settings.export_format();
        self.export_settings = settings;
    }

    fn build_export_settings_for_project(
        project: &Project,
        composition: &Composition,
    ) -> ExportSettings {
        let mut settings = ExportSettings::for_dimensions(
            composition.width as u32,
            composition.height as u32,
            composition.fps,
        );

        let config = &project.export;
        if config.container.is_none()
            && config.codec.is_none()
            && config.pixel_format.is_none()
            && config.ffmpeg_path.is_none()
            && config.parameters.is_empty()
        {
            return settings;
        }

        if let Some(value) = &config.container {
            settings.container = value.clone();
        }
        if let Some(value) = &config.codec {
            settings.codec = value.clone();
        }
        if let Some(value) = &config.pixel_format {
            settings.pixel_format = value.clone();
        }
        if let Some(value) = &config.ffmpeg_path {
            settings.ffmpeg_path = Some(value.clone());
        }
        settings.parameters = config.parameters.clone();

        if matches!(settings.export_format(), ExportFormat::Video) {
            if settings.codec == "png" {
                settings.codec = "libx264".into();
            }
            if settings.pixel_format == "rgba" {
                settings.pixel_format = "yuv420p".into();
            }
        }

        settings
    }
}
