use std::cmp;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

use log::{error, info};

use crate::Image;
use crate::framing::{PropertyEvaluatorRegistry, get_frame_from_project};
use crate::model::project::project::{Composition, Project};
use crate::plugin::{ExportFormat, PluginManager};
use crate::rendering::RenderContext;
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::util::timing::{ScopedTimer, measure_debug, measure_info};

#[derive(Debug)]
struct SaveTask {
  frame_index: u64,
  output_path: String,
  image: Image,
}

#[derive(Debug)]
struct RenderWorkerContext {
  composition_index: usize,
  surface_width: u32,
  surface_height: u32,
  background_color: crate::model::frame::color::Color,
  export_format: ExportFormat,
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
  pub export_format: ExportFormat,
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
  save_tx: Option<mpsc::SyncSender<SaveTask>>,
  workers: Vec<JoinHandle<()>>,
  saver_handle: Option<JoinHandle<()>>,
}

impl RenderQueue {
  pub fn new(config: RenderQueueConfig, total_frames: u64) -> Result<Self, RenderQueueError> {
    let composition = Self::composition_for(&config)?;
    let worker_context = Arc::new(RenderWorkerContext::from_composition(
      &composition,
      config.composition_index,
      config.export_format,
    ));

    let (save_tx, saver_handle) = Self::spawn_saver(
      Arc::clone(&config.plugin_manager),
      Arc::clone(&worker_context),
      config.save_queue_bound(),
    );
    let (render_tx, workers) =
      Self::spawn_workers(&config, total_frames, Arc::clone(&worker_context), &save_tx);

    Ok(Self {
      render_tx: Some(render_tx),
      save_tx: Some(save_tx),
      workers,
      saver_handle: Some(saver_handle),
    })
  }

  pub fn submit(&self, job: FrameRenderJob) -> Result<(), RenderQueueError> {
    let sender = self
      .render_tx
      .as_ref()
      .ok_or(RenderQueueError::QueueClosed)?;
    sender.send(job).map_err(|_| RenderQueueError::SubmitFailed)
  }

  pub fn finish(mut self) -> Result<(), RenderQueueError> {
    self.shutdown()
  }

  fn shutdown(&mut self) -> Result<(), RenderQueueError> {
    if let Some(sender) = self.render_tx.take() {
      drop(sender);
    }

    for handle in self.workers.drain(..) {
      handle
        .join()
        .map_err(|_| RenderQueueError::WorkerPanicked)?;
    }

    if let Some(sender) = self.save_tx.take() {
      drop(sender);
    }

    if let Some(handle) = self.saver_handle.take() {
      handle.join().map_err(|_| RenderQueueError::SaverPanicked)?;
    }

    Ok(())
  }

  fn composition_for(config: &RenderQueueConfig) -> Result<Composition, RenderQueueError> {
    config
      .project
      .compositions
      .get(config.composition_index)
      .cloned()
      .ok_or(RenderQueueError::InvalidCompositionIndex(
        config.composition_index,
      ))
  }

  fn spawn_saver(
    plugin_manager: Arc<PluginManager>,
    worker_ctx: Arc<RenderWorkerContext>,
    queue_bound: usize,
  ) -> (mpsc::SyncSender<SaveTask>, JoinHandle<()>) {
    let (save_tx, save_rx) = mpsc::sync_channel::<SaveTask>(cmp::max(1, queue_bound));
    let saver_handle = thread::spawn(move || {
      while let Ok(task) = save_rx.recv() {
        if let Err(err) = measure_info(format!("Frame {}: save image", task.frame_index), || {
          plugin_manager.export_image(worker_ctx.export_format, &task.output_path, &task.image)
        }) {
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
    worker_context: Arc<RenderWorkerContext>,
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
      let property_evaluators = Arc::clone(&config.property_evaluators);
      let project = Arc::clone(&config.project);
      let render_rx = Arc::clone(&render_rx);
      let save_tx = save_tx.clone();
      let ctx = Arc::clone(&worker_context);

      let handle = thread::spawn(move || {
        let mut render_context = RenderContext::new(
          SkiaRenderer::new(
            ctx.surface_width,
            ctx.surface_height,
            ctx.background_color.clone(),
          ),
          plugin_manager,
          Arc::clone(&property_evaluators),
        );

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
          let _frame_scope = ScopedTimer::info(format!(
            "Frame {} total (worker {})",
            job.frame_index, worker_id
          ));

          if let Err(err) = measure_debug(
            format!("Frame {}: clear render target", job.frame_index),
            || render_context.clear(),
          ) {
            error!(
              "Worker {} failed to clear render target for frame {}: {}",
              worker_id, job.frame_index, err
            );
            continue;
          }

          let frame = measure_debug(
            format!("Frame {}: assemble frame graph", job.frame_index),
            || {
              get_frame_from_project(
                project.as_ref(),
                ctx.composition_index,
                job.frame_time,
                &property_evaluators,
              )
            },
          );

          let render_result = measure_info(
            format!(
              "Frame {}: renderer pass (worker {})",
              job.frame_index, worker_id
            ),
            || render_context.render_frame(frame),
          );

          let image = match render_result {
            Ok(img) => img,
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
    let _ = self.shutdown();
  }
}

#[derive(Debug)]
pub enum RenderQueueError {
  InvalidCompositionIndex(usize),
  QueueClosed,
  SubmitFailed,
  WorkerPanicked,
  SaverPanicked,
}

impl std::fmt::Display for RenderQueueError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      RenderQueueError::InvalidCompositionIndex(idx) => {
        write!(f, "invalid composition index {}", idx)
      }
      RenderQueueError::QueueClosed => write!(f, "render queue already closed"),
      RenderQueueError::SubmitFailed => write!(f, "failed to submit job to render queue"),
      RenderQueueError::WorkerPanicked => write!(f, "render worker thread panicked"),
      RenderQueueError::SaverPanicked => write!(f, "save worker thread panicked"),
    }
  }
}

impl std::error::Error for RenderQueueError {}

impl RenderWorkerContext {
  fn from_composition(
    composition: &Composition,
    composition_index: usize,
    export_format: ExportFormat,
  ) -> Self {
    Self {
      composition_index,
      surface_width: composition.width as u32,
      surface_height: composition.height as u32,
      background_color: composition.background_color.clone(),
      export_format,
    }
  }
}
