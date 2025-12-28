use crate::Image;
use crate::editor::{ProjectModel, RenderService};
use crate::model::project::project::{Composition, Project};
use crate::plugin::{ExportFormat, ExportSettings, PluginManager};
// use crate::rendering::effects::EffectRegistry; // Removed
use crate::error::LibraryError;
use crate::framing::entity_converters::EntityConverterRegistry;
use crate::plugin::PropertyEvaluatorRegistry;
use crate::rendering::renderer::Renderer;
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::util::timing::{ScopedTimer, measure_info_lazy};
use log::{error, info};
use std::cmp;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use tokio::task; // Corrected tokio::task import

#[derive(Debug)]
struct SaveTask {
    frame_index: u64,
    output_path: String,
    image: Image,
    #[allow(dead_code)]
    export_settings: Arc<ExportSettings>,
}

#[derive(Debug)]
pub struct FrameRenderJob {
    pub frame_index: u64,
    pub frame_time: f64,
    pub output_path: String,
}

impl FrameRenderJob {
    pub fn new(frame_index: u64, frame_time: f64, output_path: impl Into<String>) -> Self {
        Self {
            frame_index,
            frame_time,
            output_path: output_path.into(),
        }
    }
}

pub struct RenderQueueConfig {
    pub project: Arc<Project>,
    pub composition_index: usize,
    pub plugin_manager: Arc<PluginManager>,
    pub property_evaluators: Arc<PropertyEvaluatorRegistry>,
    pub cache_manager: Arc<crate::cache::CacheManager>, // Added this line
    // pub effect_registry: Arc<EffectRegistry>, // Removed
    pub export_format: ExportFormat,
    pub export_settings: Arc<ExportSettings>,
    pub entity_converter_registry: Arc<EntityConverterRegistry>,
    pub worker_count: Option<usize>,
    pub save_queue_bound: usize,
}

impl RenderQueueConfig {
    fn worker_count(&self, total_frames: u64) -> usize {
        if let Some(count) = self.worker_count {
            return cmp::max(1, count);
        }
        let logical = thread::available_parallelism()
            .map(|v| v.get())
            .unwrap_or(1);
        cmp::max(1, cmp::min(logical, cmp::max(1, total_frames as usize)))
    }

    fn save_queue_bound(&self) -> usize {
        cmp::max(1, self.save_queue_bound)
    }
}

pub struct RenderQueue {
    render_tx: Option<mpsc::Sender<FrameRenderJob>>,
    #[allow(dead_code)]
    save_tx: Option<mpsc::SyncSender<SaveTask>>,
    workers: Vec<JoinHandle<()>>,
    saver_handle: Option<JoinHandle<()>>,
}

impl RenderQueue {
    pub fn new(config: RenderQueueConfig, total_frames: u64) -> Result<Self, LibraryError> {
        let composition = Self::composition_for(&config)?;
        let (save_tx, saver_handle) = Self::spawn_saver(
            Arc::clone(&config.plugin_manager),
            config.export_format,
            Arc::clone(&config.export_settings),
            config.save_queue_bound(),
        );
        let (render_tx, workers) = Self::spawn_workers(
            &config,
            total_frames,
            config.composition_index,
            composition.width as u32,
            composition.height as u32,
            composition.background_color.clone(),
            config.export_format,
            Arc::clone(&config.export_settings),
            Arc::clone(&config.cache_manager),
            Arc::clone(&config.entity_converter_registry),
            &save_tx,
        );

        Ok(Self {
            render_tx: Some(render_tx),
            save_tx: Some(save_tx),
            workers,
            saver_handle: Some(saver_handle),
        })
    }

    pub fn submit(&self, job: FrameRenderJob) -> Result<(), LibraryError> {
        let sender = self
            .render_tx
            .as_ref()
            .ok_or(LibraryError::RenderQueueClosed)?;
        sender
            .send(job)
            .map_err(|_| LibraryError::RenderSubmitFailed)
    }

    pub async fn finish(mut self) -> Result<(), LibraryError> {
        // Made async
        self.shutdown().await
    }

    async fn shutdown(&mut self) -> Result<(), LibraryError> {
        // Made async
        if let Some(sender) = self.render_tx.take() {
            drop(sender);
        }

        for handle in self.workers.drain(..) {
            task::spawn_blocking(move || handle.join())
                .await
                .map_err(|_| LibraryError::RenderWorkerPanicked)? // Error from spawn_blocking
                .map_err(|_| LibraryError::RenderWorkerPanicked)?; // Error from join()
        }

        if let Some(handle) = self.saver_handle.take() {
            task::spawn_blocking(move || handle.join())
                .await
                .map_err(|_| LibraryError::RenderSaverPanicked)? // Error from spawn_blocking
                .map_err(|_| LibraryError::RenderSaverPanicked)?; // Error from join()
        }

        Ok(())
    }

    fn composition_for(config: &RenderQueueConfig) -> Result<Composition, LibraryError> {
        config
            .project
            .compositions
            .get(config.composition_index)
            .cloned()
            .ok_or(LibraryError::InvalidCompositionIndex(
                config.composition_index,
            ))
    }

    fn spawn_saver(
        plugin_manager: Arc<PluginManager>,
        _export_format: ExportFormat,
        export_settings: Arc<ExportSettings>,
        queue_bound: usize,
    ) -> (mpsc::SyncSender<SaveTask>, JoinHandle<()>) {
        let (save_tx, save_rx) = mpsc::sync_channel::<SaveTask>(cmp::max(1, queue_bound));
        let saver_handle = thread::spawn(move || {
            while let Ok(task) = save_rx.recv() {
                if let Err(err) = measure_info_lazy(
                    || format!("Frame {}: save image", task.frame_index),
                    || {
                        plugin_manager.export_image(
                            "png_export", // Hardcoded exporter_id
                            &task.output_path,
                            &task.image,
                            &export_settings,
                        )
                    },
                ) {
                    error!("Failed to save frame {}: {}", task.frame_index, err);
                    break;
                }
            }
        });
        (save_tx, saver_handle)
    }

    fn spawn_workers(
        config: &RenderQueueConfig,
        total_frames: u64,
        composition_index: usize,
        surface_width: u32,
        surface_height: u32,
        background_color: crate::model::frame::color::Color,
        _export_format: ExportFormat,
        export_settings: Arc<ExportSettings>,
        cache_manager: Arc<crate::cache::CacheManager>, // Added this line
        _entity_converter_registry: Arc<EntityConverterRegistry>,
        save_tx: &mpsc::SyncSender<SaveTask>,
    ) -> (mpsc::Sender<FrameRenderJob>, Vec<JoinHandle<()>>) {
        let (render_tx, render_rx) = mpsc::channel::<FrameRenderJob>();
        let render_rx = Arc::new(Mutex::new(render_rx));

        let worker_count = config.worker_count(total_frames);
        info!(
            "RenderQueue starting {} worker(s) for {} frame(s)",
            worker_count, total_frames
        );

        let mut workers = Vec::with_capacity(worker_count);
        for worker_id in 0..worker_count {
            let plugin_manager = Arc::clone(&config.plugin_manager);
            let _property_evaluators = Arc::clone(&config.property_evaluators);
            let project = Arc::clone(&config.project);
            let render_rx = Arc::clone(&render_rx);
            let save_tx = save_tx.clone();
            // Moved ctx fields into individual captures
            let background_color_clone = background_color.clone();
            let export_settings_clone = Arc::clone(&export_settings);
            let entity_converter_registry_clone = Arc::clone(&config.entity_converter_registry);

            let cache_manager_for_thread = Arc::clone(&cache_manager);

            let handle = thread::spawn(move || {
                let mut render_service = RenderService::new(
                    SkiaRenderer::new(
                        surface_width,
                        surface_height,
                        background_color_clone,
                        false,
                        None,
                    ),
                    plugin_manager,
                    cache_manager_for_thread,
                    entity_converter_registry_clone,
                );

                let project_model = match ProjectModel::new(project, composition_index) {
                    Ok(model) => model,
                    Err(err) => {
                        error!(
                            "Worker {} failed to create project model: {}",
                            worker_id, err
                        );
                        return; // Exit worker thread
                    }
                };

                loop {
                    let job = {
                        let receiver = render_rx.lock().expect("render queue poisoned");
                        receiver.recv()
                    };

                    let job = match job {
                        Ok(job) => job,
                        Err(_) => break,
                    };

                    info!("Worker {} rendering frame {}", worker_id, job.frame_index);
                    let _frame_scope = ScopedTimer::info_lazy(|| {
                        format!(
                            "Frame {} total (worker {})",
                            job.frame_index, worker_id
                        )
                    });

                    let render_result = measure_info_lazy(
                        || {
                            format!(
                                "Frame {}: renderer pass (worker {})",
                                job.frame_index, worker_id
                            )
                        },
                        || render_service.render_frame(&project_model, job.frame_time),
                    );

                    let image = match render_result {
                        Ok(crate::rendering::renderer::RenderOutput::Image(img)) => img,
                        Ok(output @ crate::rendering::renderer::RenderOutput::Texture(_)) => {
                            match render_service.renderer.read_surface(&output) {
                                Ok(img) => img,
                                Err(err) => {
                                    error!(
                                        "Worker {} failed to read texture from surface: {}",
                                        worker_id, err
                                    );
                                    continue;
                                }
                            }
                        }
                        Err(err) => {
                            error!(
                                "Worker {} failed to render frame {}: {}",
                                worker_id, job.frame_index, err
                            );
                            continue;
                        }
                    };

                    if let Err(err) = save_tx.send(SaveTask {
                        frame_index: job.frame_index,
                        output_path: job.output_path,
                        image,
                        export_settings: export_settings_clone.clone(),
                    }) {
                        error!(
                            "Worker {} failed to queue save task for frame {}: {}",
                            worker_id, job.frame_index, err
                        );
                        break;
                    }
                }
            });
            workers.push(handle);
        }

        (render_tx, workers)
    }
}

impl Drop for RenderQueue {
    fn drop(&mut self) {
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(self.shutdown()); // Changed to use async shutdown
    }
}
