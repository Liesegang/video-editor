use crate::core::rendering::renderer::Renderer;
use crate::editor::project_model::ProjectModel;
use crate::editor::render_service::RenderService;
use crate::error::LibraryError;
use crate::model::project::Project;
use crate::plugin::{
    ExportColorAuthority, ExportDestination, ExportFormat, ExportFrame, ExportSettings,
    PluginManager,
};
use crate::util::output_path_identity::{OutputPathIdentity, output_path_identity};
use crate::util::timing::{ScopedTimer, measure_info};
use log::{error, info};

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

struct SaveTask {
    exporter_id: String,
    frame_index: u64,
    output_path: String,
    frame: ExportFrame,
    export_settings: Arc<ExportSettings>,
    source_aliases: Arc<ProjectSourceAliases>,
}

#[derive(Clone, Debug, PartialEq)]
struct PlannedExportSettings {
    container: String,
    codec: String,
    pixel_format: String,
    trusted_ffmpeg_path: Option<String>,
    width: u32,
    height: u32,
    fps_bits: u64,
    parameters: HashMap<String, serde_json::Value>,
    color_authority: ExportColorAuthority,
}

impl PlannedExportSettings {
    fn capture(settings: &ExportSettings, project: &Project) -> Result<Self, LibraryError> {
        let expected_authority = ExportColorAuthority::from_project(project)?;
        let actual_authority = settings.color_authority().ok_or_else(|| {
            LibraryError::Render(
                "export planning requires Project-derived color authority".to_string(),
            )
        })?;
        if actual_authority != &expected_authority {
            return Err(LibraryError::Render(
                "export settings color authority does not belong to the planned Project snapshot"
                    .to_string(),
            ));
        }
        Ok(Self {
            container: settings.container.clone(),
            codec: settings.codec.clone(),
            pixel_format: settings.pixel_format.clone(),
            trusted_ffmpeg_path: settings.trusted_ffmpeg_path().map(str::to_string),
            width: settings.width,
            height: settings.height,
            fps_bits: settings.fps.to_bits(),
            parameters: settings.parameters.clone(),
            color_authority: expected_authority,
        })
    }

    fn require_match(&self, settings: &ExportSettings) -> Result<(), LibraryError> {
        if self.container != settings.container
            || self.codec != settings.codec
            || self.pixel_format != settings.pixel_format
            || self.trusted_ffmpeg_path.as_deref() != settings.trusted_ffmpeg_path()
            || self.width != settings.width
            || self.height != settings.height
            || self.fps_bits != settings.fps.to_bits()
            || self.parameters != settings.parameters
            || settings.color_authority() != Some(&self.color_authority)
        {
            return Err(LibraryError::Render(
                "output-affecting export settings changed after the output plan was verified; create a new plan"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct ProjectSourceAlias {
    asset_name: String,
    source_path: String,
    identity: OutputPathIdentity,
}

#[derive(Clone, Debug, Default)]
struct ProjectSourceAliases {
    entries: Vec<ProjectSourceAlias>,
}

impl ProjectSourceAliases {
    fn capture(project: &Project) -> Result<Self, LibraryError> {
        let mut entries = Vec::new();
        for asset in &project.assets {
            if asset.path.trim().is_empty() {
                // Placeholder/generated Assets have no external source to
                // protect. In particular, an empty path must not resolve to
                // the process working directory.
                continue;
            }
            match std::fs::symlink_metadata(&asset.path) {
                Ok(metadata)
                    if !metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
                {
                    // A directory, socket, FIFO, or device is not a file
                    // source. Automatic media loading rejects it separately.
                    continue;
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // Retain lexical protection for a temporarily missing or
                    // relocated source path.
                }
                Err(error) => {
                    return Err(LibraryError::Render(format!(
                        "cannot inspect Project asset '{}' at '{}': {error}",
                        asset.name, asset.path
                    )));
                }
            }
            let identity = output_path_identity(&asset.path).map_err(|error| {
                LibraryError::Render(format!(
                    "cannot establish the export-safe identity of Project asset '{}' at '{}': {error}",
                    asset.name, asset.path
                ))
            })?;
            entries.push(ProjectSourceAlias {
                asset_name: asset.name.clone(),
                source_path: asset.path.clone(),
                identity,
            });
        }
        Ok(Self { entries })
    }

    fn require_safe_output(&self, output_path: &str) -> Result<(), LibraryError> {
        let output_identity = output_path_identity(output_path).map_err(|error| {
            LibraryError::Render(format!(
                "cannot establish export destination identity for '{output_path}': {error}"
            ))
        })?;
        if let Some(source) = self
            .entries
            .iter()
            .find(|source| source.identity.aliases(&output_identity))
        {
            return Err(LibraryError::Render(format!(
                "export destination '{output_path}' aliases Project asset '{}' at '{}'; refusing to overwrite an input source",
                source.asset_name, source.source_path
            )));
        }
        Ok(())
    }
}

/// Opaque proof that every destination in one complete selected output range
/// was checked against the exact Project snapshot before a save worker exists.
///
/// The plan is consumed by [`ExportService::new`]. Runtime-created audio may
/// be bound to settings after planning; every other output-affecting setting
/// must remain identical.
pub struct VerifiedExportPlan {
    project: Arc<Project>,
    composition_index: usize,
    frame_range: Range<u64>,
    base_template: String,
    has_frame_token: bool,
    export_format: ExportFormat,
    video_output: Option<String>,
    settings: PlannedExportSettings,
    source_aliases: Arc<ProjectSourceAliases>,
}

#[derive(Default)]
struct AttemptedOutputs {
    paths: Vec<String>,
    seen: HashSet<String>,
}

impl AttemptedOutputs {
    fn record(&mut self, path: &str) {
        if self.seen.insert(path.to_string()) {
            self.paths.push(path.to_string());
        }
    }

    fn take_paths(&mut self) -> Vec<String> {
        self.seen.clear();
        std::mem::take(&mut self.paths)
    }
}

pub struct ExportService {
    save_tx: Option<SyncSender<SaveTask>>,
    saver_handle: Option<JoinHandle<()>>,
    export_settings: Arc<ExportSettings>,
    exporter_id: String,
    temp_files: Vec<String>,
    save_error: Arc<Mutex<Option<String>>>,
    plugin_manager: Arc<PluginManager>,
    video_output_path: Option<String>,
    queued_frames: u64,
    attempted_outputs: Arc<Mutex<AttemptedOutputs>>,
    plan: VerifiedExportPlan,
}

impl ExportService {
    /// Verify the complete selected output set without creating a save worker,
    /// opening an output, or invoking an exporter.
    pub fn verify_plan(
        project_model: &ProjectModel,
        export_settings: &ExportSettings,
        selected_frame_range: Range<u64>,
        output_stem: &str,
    ) -> Result<VerifiedExportPlan, LibraryError> {
        let project = Arc::clone(project_model.project());
        let composition = project_model.composition();
        let frame_range = export_settings
            .frame_range_within_duration(selected_frame_range, composition.duration)?;
        let base_template = Self::resolve_template(output_stem, &project.name, &composition.name);
        let has_frame_token = base_template.contains("{frame");
        let export_format = export_settings.export_format();
        let video_output = if matches!(export_format, ExportFormat::Video) {
            Some(Self::video_output_path(
                &base_template,
                &export_settings.container,
            )?)
        } else {
            None
        };
        let settings = PlannedExportSettings::capture(export_settings, project.as_ref())?;
        let source_aliases = Arc::new(ProjectSourceAliases::capture(project.as_ref())?);

        match export_format {
            ExportFormat::Video => {
                source_aliases.require_safe_output(video_output.as_deref().ok_or_else(|| {
                    LibraryError::Render("video output path was not planned".to_string())
                })?)?
            }
            ExportFormat::Png => {
                for frame_index in frame_range.clone() {
                    let output_path =
                        Self::png_output_path(&base_template, has_frame_token, frame_index);
                    source_aliases.require_safe_output(&output_path)?;
                }
            }
        }

        Ok(VerifiedExportPlan {
            project,
            composition_index: project_model.composition_index(),
            frame_range,
            base_template,
            has_frame_token,
            export_format,
            video_output,
            settings,
            source_aliases,
        })
    }

    pub fn new(
        plugin_manager: Arc<PluginManager>,
        exporter_id: String,
        export_settings: Arc<ExportSettings>,
        plan: VerifiedExportPlan,
        save_queue_bound: usize,
    ) -> Result<Self, LibraryError> {
        plan.settings.require_match(export_settings.as_ref())?;
        let mut job_settings = (*export_settings).clone();
        job_settings.begin_new_job();
        let export_settings = Arc::new(job_settings);
        let queue_bound = save_queue_bound.max(1);
        let (save_tx, save_rx) = mpsc::sync_channel::<SaveTask>(queue_bound);
        let save_error = Arc::new(Mutex::new(None));
        let worker_error = Arc::clone(&save_error);
        let worker_plugins = Arc::clone(&plugin_manager);
        let attempted_outputs = Arc::new(Mutex::new(AttemptedOutputs::default()));
        let worker_attempted_outputs = Arc::clone(&attempted_outputs);
        let saver_handle = thread::spawn(move || {
            while let Ok(task) = save_rx.recv() {
                if let Err(err) = task.source_aliases.require_safe_output(&task.output_path) {
                    error!(
                        "Refusing export frame {} to {} after destination identity changed: {}",
                        task.frame_index, task.output_path, err
                    );
                    *worker_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!(
                        "refused export frame {} to {} before exporter callback: {err}",
                        task.frame_index, task.output_path
                    ));
                    break;
                }
                worker_attempted_outputs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record(&task.output_path);
                let destination = ExportDestination::staged(&task.output_path, &task.output_path);
                if let Err(err) = worker_plugins.export_frame(
                    &task.exporter_id,
                    &destination,
                    &task.frame,
                    &task.export_settings,
                ) {
                    error!(
                        "Failed to save frame {} to {}: {}",
                        task.frame_index, task.output_path, err
                    );
                    *worker_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!(
                        "failed to save frame {} to {}: {err}",
                        task.frame_index, task.output_path
                    ));
                    break;
                }
            }
        });

        Ok(Self {
            save_tx: Some(save_tx),
            saver_handle: Some(saver_handle),
            export_settings,
            exporter_id,
            temp_files: Vec::new(),
            save_error,
            plugin_manager,
            video_output_path: None,
            queued_frames: 0,
            attempted_outputs,
            plan,
        })
    }

    pub fn render_range<T: Renderer>(
        &mut self,
        render_service: &mut RenderService<T>,
        project_model: &ProjectModel,
        frame_range: Range<u64>,
    ) -> Result<(), LibraryError> {
        self.check_save_error()?;
        if !Arc::ptr_eq(project_model.project(), &self.plan.project)
            || project_model.composition_index() != self.plan.composition_index
        {
            return Err(LibraryError::Render(
                "export chunk does not use the Project snapshot and composition bound to its verified output plan"
                    .to_string(),
            ));
        }
        if frame_range.start >= frame_range.end
            || frame_range.start < self.plan.frame_range.start
            || frame_range.end > self.plan.frame_range.end
        {
            return Err(LibraryError::Render(format!(
                "export chunk {}..{} is outside verified selection {}..{}",
                frame_range.start,
                frame_range.end,
                self.plan.frame_range.start,
                self.plan.frame_range.end
            )));
        }

        if let Some(path) = &self.plan.video_output {
            match &self.video_output_path {
                Some(active_path) if active_path != path => {
                    return Err(LibraryError::Render(format!(
                        "one ExportService cannot change video output from '{active_path}' to '{path}' between chunks"
                    )));
                }
                Some(_) => {}
                None => self.video_output_path = Some(path.clone()),
            }
        }

        let settings_arc = Arc::clone(&self.export_settings);
        let sender = self.save_tx.as_ref().ok_or(LibraryError::Render(
            "Save queue is already closed".to_string(),
        ))?;

        for frame_index in frame_range {
            info!("Render frame {}:", frame_index);
            let _frame_scope = ScopedTimer::info(format!("Frame {} total", frame_index));

            let frame_time = settings_arc.frame_time(frame_index)?;
            let output = measure_info(format!("Frame {}: renderer pass", frame_index), || {
                render_service.render_export_frame(project_model, frame_time)
            })?;

            settings_arc.require_matching_color_authority(&output)?;

            let output_path = match self.plan.export_format {
                ExportFormat::Png => Self::png_output_path(
                    &self.plan.base_template,
                    self.plan.has_frame_token,
                    frame_index,
                ),
                ExportFormat::Video => {
                    self.plan.video_output.as_ref().cloned().ok_or_else(|| {
                        LibraryError::Render("Video output path was not initialized".to_string())
                    })?
                }
            };
            sender
                .send(SaveTask {
                    exporter_id: self.exporter_id.clone(),
                    frame_index,
                    output_path,
                    frame: output,
                    export_settings: Arc::clone(&settings_arc), // Use the modified settings
                    source_aliases: Arc::clone(&self.plan.source_aliases),
                })
                .map_err(|_| LibraryError::Render("Save queue disconnected".to_string()))?;
            self.queued_frames = self.queued_frames.checked_add(1).ok_or_else(|| {
                LibraryError::Render("export queued-frame count overflowed".to_string())
            })?;
            self.check_save_error()?;
        }

        Ok(())
    }

    fn png_output_path(base_template: &str, has_frame_token: bool, frame_index: u64) -> String {
        if has_frame_token {
            let name = Self::format_frame_token_in_string(base_template, frame_index);
            Self::with_extension(&name, "png")
        } else {
            Self::numbered_path(base_template, frame_index, "png")
        }
    }

    fn has_extension(path: &str, extension: &str) -> bool {
        let extension = extension.trim_start_matches('.');
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    }

    fn with_extension(path: &str, extension: &str) -> String {
        let extension = extension.trim_start_matches('.');
        if extension.is_empty() || Self::has_extension(path, extension) {
            path.to_string()
        } else {
            format!("{path}.{extension}")
        }
    }

    fn numbered_path(path: &str, frame: u64, extension: &str) -> String {
        let extension = extension.trim_start_matches('.');
        if !Self::has_extension(path, extension) {
            return format!("{path}_{frame:03}.{extension}");
        }

        // Preserve the separator spelling from the user-authored template.
        // `Path::with_file_name` rewrites `/` to `\` on Windows, which makes
        // otherwise identical export plans platform-dependent.
        let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let prefix = path.strip_suffix(file_name).unwrap_or_default();
        let stem = Path::new(file_name)
            .file_stem()
            .map(|value| value.to_string_lossy())
            .unwrap_or_default();
        format!("{prefix}{stem}_{frame:03}.{extension}")
    }

    fn video_output_path(path: &str, container: &str) -> Result<String, LibraryError> {
        if path.contains("{frame") {
            return Err(LibraryError::Render(
                "video export does not support {frame} path tokens; one output must retain one explicitly finalized FFmpeg session"
                    .to_string(),
            ));
        }
        Ok(Self::with_extension(path, container))
    }

    fn resolve_template(template: &str, project: &str, composition: &str) -> String {
        template
            .replace("{project}", project)
            .replace("{composition}", composition)
    }

    pub fn shutdown(mut self) -> Result<(), LibraryError> {
        self.shutdown_inner(true)
    }

    /// Drain and finalize a cancelled job without misreporting an intentional
    /// zero-frame cancellation as a successful empty export.
    pub fn cancel(mut self) -> Result<(), LibraryError> {
        self.shutdown_inner(false)
    }

    fn shutdown_inner(&mut self, require_output: bool) -> Result<(), LibraryError> {
        self.save_tx.take();
        let join_result = self.saver_handle.take().map_or(Ok(()), |handle| {
            handle
                .join()
                .map_err(|_| LibraryError::Render("Failed to join save worker".to_string()))
        });
        let save_result = self.check_save_error();
        let output_result = if require_output {
            self.require_output_enqueued()
        } else {
            Ok(())
        };
        let finish_result = self.finish_attempted_outputs();
        join_result
            .and(save_result)
            .and(output_result)
            .and(finish_result)
    }

    fn finish_attempted_outputs(&mut self) -> Result<(), LibraryError> {
        let paths = self
            .attempted_outputs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_paths();
        let mut first_error = None;
        for path in paths {
            let destination = ExportDestination::staged(&path, &path);
            if let Err(error) = self.plugin_manager.finish_export(
                &self.exporter_id,
                &destination,
                &self.export_settings,
            ) && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn require_output_enqueued(&self) -> Result<(), LibraryError> {
        if self.queued_frames == 0 {
            Err(LibraryError::Render(
                "export completed without queueing any output frames".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn check_save_error(&self) -> Result<(), LibraryError> {
        self.save_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map_or(Ok(()), |error| Err(LibraryError::Render(error.clone())))
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
                let check_chars = chars.clone();
                for tc in check_chars {
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
                    } else if let Some(spec) = token_buffer.strip_prefix("frame:") {
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
        if let Some(handle) = self.saver_handle.take()
            && handle.join().is_err()
        {
            error!("Save worker panicked during ExportService shutdown");
        }
        if let Err(error) = self.check_save_error() {
            error!("ExportService save worker failed during drop: {error}");
        }
        if let Err(error) = self.finish_attempted_outputs() {
            error!("ExportService failed to finalize attempted output during drop: {error}");
        }
        for path in &self.temp_files {
            if let Err(e) = std::fs::remove_file(path) {
                error!("Failed to remove temp file {}: {}", path, e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ExportService, SaveTask};
    use crate::editor::ProjectModel;
    use crate::error::LibraryError;
    use crate::model::frame::Image;
    use crate::model::project::{Composition, Project};
    use crate::plugin::{
        ExportDestination, ExportFrame, ExportPlugin, ExportSettings, Plugin, PluginManager,
    };
    use std::sync::{Arc, Mutex};

    struct RecordingExporter {
        events: Arc<Mutex<Vec<String>>>,
        fail_finish_path: Option<String>,
    }

    impl Plugin for RecordingExporter {
        fn id(&self) -> &'static str {
            "recording_export"
        }

        fn name(&self) -> String {
            "Recording Export".to_string()
        }

        fn category(&self) -> String {
            "Export".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl ExportPlugin for RecordingExporter {
        fn export_frame(
            &self,
            destination: &ExportDestination,
            _frame: &ExportFrame,
            _settings: &ExportSettings,
        ) -> Result<(), LibraryError> {
            self.events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!("export:{}", destination.logical_path()));
            Ok(())
        }

        fn finish_export(
            &self,
            destination: &ExportDestination,
            _settings: &ExportSettings,
        ) -> Result<(), LibraryError> {
            let path = destination.logical_path();
            self.events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!("finish:{path}"));
            if self.fail_finish_path.as_deref() == Some(path) {
                Err(LibraryError::Render(format!(
                    "intentional finish failure for {path}"
                )))
            } else {
                Ok(())
            }
        }
    }

    fn recording_service(
        fail_finish_path: Option<&str>,
    ) -> (ExportService, Arc<Mutex<Vec<String>>>, Arc<ExportSettings>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let manager = Arc::new(PluginManager::new());
        manager.register_export_plugin(Arc::new(RecordingExporter {
            events: Arc::clone(&events),
            fail_finish_path: fail_finish_path.map(str::to_string),
        }));
        let mut project = Project::new("export lifecycle");
        let (composition, track) = Composition::new("main", 1, 1, 24.0, 1.0);
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        let mut settings = ExportSettings::for_dimensions(1, 1, 24.0);
        settings.bind_project_color_authority(&project).unwrap();
        let model = ProjectModel::new(Arc::new(project), 0).unwrap();
        let plan =
            ExportService::verify_plan(&model, &settings, 0..2, "render/export-lifecycle").unwrap();
        let settings = Arc::new(settings);
        (
            ExportService::new(
                manager,
                "recording_export".to_string(),
                Arc::clone(&settings),
                plan,
                2,
            )
            .unwrap(),
            events,
            settings,
        )
    }

    fn send_test_frame(service: &mut ExportService, settings: &Arc<ExportSettings>, path: &str) {
        let project = Project::new("export lifecycle");
        let frame =
            ExportFrame::from_project_render(&project, Image::new(1, 1, vec![10, 20, 30, 255]))
                .unwrap();
        service
            .save_tx
            .as_ref()
            .unwrap()
            .send(SaveTask {
                exporter_id: "recording_export".to_string(),
                frame_index: service.queued_frames,
                output_path: path.to_string(),
                frame,
                export_settings: Arc::clone(settings),
                source_aliases: Arc::clone(&service.plan.source_aliases),
            })
            .unwrap();
        service.queued_frames += 1;
    }

    #[test]
    fn export_paths_handle_single_sequence_token_and_existing_extensions() {
        assert_eq!(
            ExportService::with_extension("render/final", "png"),
            "render/final.png"
        );
        assert_eq!(
            ExportService::with_extension("render/final.png", "png"),
            "render/final.png"
        );
        assert_eq!(
            ExportService::numbered_path("render/final", 12, "png"),
            "render/final_012.png"
        );
        assert_eq!(
            ExportService::numbered_path("render/final.png", 12, "png"),
            "render/final_012.png"
        );
        assert_eq!(
            ExportService::with_extension("render/final.mp4", "mp4"),
            "render/final.mp4"
        );
        assert_eq!(
            ExportService::with_extension("render/final", ".mp4"),
            "render/final.mp4"
        );
        assert_eq!(
            ExportService::with_extension(
                &ExportService::format_frame_token_in_string("render/final_{frame:04}.png", 7,),
                "png",
            ),
            "render/final_0007.png"
        );
    }

    #[test]
    fn video_path_is_stable_across_chunks_and_rejects_frame_tokens() {
        assert_eq!(
            ExportService::video_output_path("render/final", "mp4").unwrap(),
            "render/final.mp4"
        );
        assert_eq!(
            ExportService::video_output_path("render/final", "mp4").unwrap(),
            "render/final.mp4",
            "a later render_range chunk must address the same FFmpeg session"
        );
        let error = ExportService::video_output_path("render/out_{frame:04}", "mp4").unwrap_err();
        assert!(error.to_string().contains("does not support {frame}"));
        assert!(
            error
                .to_string()
                .contains("one explicitly finalized FFmpeg session")
        );
        assert_eq!(
            ExportService::resolve_template("render/{project}/{composition}", "Project A", "Main"),
            "render/Project A/Main"
        );
    }

    #[test]
    fn attempted_paths_are_finished_exactly_once_after_all_exports() {
        let (mut service, events, settings) = recording_service(None);
        assert_ne!(
            service.export_settings.job_id(),
            settings.job_id(),
            "each ExportService must own a fresh session identity"
        );
        let first = ExportService::with_extension(
            &ExportService::resolve_template(
                "render/{project}/{composition}_000",
                "Project A",
                "Main",
            ),
            "png",
        );
        let second = first.replace("_000.png", "_001.png");
        send_test_frame(&mut service, &settings, &first);
        send_test_frame(&mut service, &settings, &second);

        service.shutdown().unwrap();

        assert_eq!(
            *events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![
                format!("export:{first}"),
                format!("export:{second}"),
                format!("finish:{first}"),
                format!("finish:{second}"),
            ]
        );
    }

    #[test]
    fn every_attempted_path_is_finished_even_when_one_finish_fails() {
        let first = "render/first.png";
        let second = "render/second.png";
        let (mut service, events, settings) = recording_service(Some(first));
        send_test_frame(&mut service, &settings, first);
        send_test_frame(&mut service, &settings, second);

        let error = service.shutdown().unwrap_err();
        assert!(error.to_string().contains("intentional finish failure"));
        assert_eq!(
            *events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![
                format!("export:{first}"),
                format!("export:{second}"),
                format!("finish:{first}"),
                format!("finish:{second}"),
            ]
        );
    }

    #[test]
    fn shutdown_cannot_report_success_without_any_output_frame() {
        let (service, _events, _settings) = recording_service(None);
        let error = service.shutdown().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("without queueing any output frames")
        );
    }

    #[test]
    fn explicit_cancellation_may_finalize_before_the_first_frame() {
        let (service, events, _settings) = recording_service(None);
        service.cancel().unwrap();
        assert!(
            events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }
}
