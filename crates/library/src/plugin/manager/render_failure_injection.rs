//! Instance-scoped, test-only failures at real render plugin boundaries.
//!
//! The hook fires only after the selected production plugin callback has
//! succeeded. It therefore verifies export cleanup without replacing the
//! Effect, Asset Loader, RenderService, or renderer with a test double.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::error::LibraryError;
use crate::model::property::PropertyValue;
use crate::plugin::LoadRequest;

use super::PluginManager;

#[derive(Clone, Copy, Debug)]
enum FailureMode {
    Error,
    Panic,
}

#[derive(Debug)]
enum FailureTarget {
    Effect {
        component_id: String,
        local_time_bits: u64,
        mode: FailureMode,
    },
    VideoLoader {
        plugin_id: String,
        path: String,
        source_time_bits: u64,
        mode: FailureMode,
    },
}

#[derive(Debug, Default)]
pub(super) struct OneShotRenderFailure {
    target: Mutex<Option<FailureTarget>>,
}

impl OneShotRenderFailure {
    fn arm(&self, target: FailureTarget) -> Result<(), LibraryError> {
        let mut armed = self.target.lock().map_err(|_| {
            LibraryError::Runtime("render failure injection lock poisoned".to_string())
        })?;
        if armed.is_some() {
            return Err(LibraryError::Runtime(
                "a render failure injection is already armed".to_string(),
            ));
        }
        *armed = Some(target);
        Ok(())
    }

    fn take_matching(
        &self,
        matches: impl FnOnce(&FailureTarget) -> bool,
    ) -> Result<Option<FailureTarget>, LibraryError> {
        let mut armed = self.target.lock().map_err(|_| {
            LibraryError::Runtime("render failure injection lock poisoned".to_string())
        })?;
        if armed.as_ref().is_some_and(matches) {
            Ok(armed.take())
        } else {
            Ok(None)
        }
    }

    pub(super) fn after_effect_success(
        &self,
        component_id: &str,
        parameters: &HashMap<String, PropertyValue>,
    ) -> Result<(), LibraryError> {
        let Some(PropertyValue::Number(local_time)) = parameters.get("u_time") else {
            return Ok(());
        };
        let local_time = local_time.into_inner();
        let Some(FailureTarget::Effect { mode, .. }) = self.take_matching(|target| {
            matches!(
                target,
                FailureTarget::Effect {
                    component_id: expected_id,
                    local_time_bits,
                    ..
                } if expected_id == component_id && *local_time_bits == local_time.to_bits()
            )
        })?
        else {
            return Ok(());
        };
        fail(
            mode,
            format!(
                "injected Effect failure after '{component_id}' succeeded at local time {local_time}"
            ),
        )
    }

    pub(super) fn after_loader_success(
        &self,
        plugin_id: &str,
        request: &LoadRequest,
    ) -> Result<(), LibraryError> {
        let LoadRequest::VideoFrame {
            path, source_time, ..
        } = request
        else {
            return Ok(());
        };
        let Some(FailureTarget::VideoLoader { mode, .. }) = self.take_matching(|target| {
            matches!(
                target,
                FailureTarget::VideoLoader {
                    plugin_id: expected_id,
                    path: expected_path,
                    source_time_bits,
                    ..
                } if expected_id == plugin_id
                    && expected_path == path
                    && *source_time_bits == source_time.to_bits()
            )
        })?
        else {
            return Ok(());
        };
        fail(
            mode,
            format!(
                "injected Asset Loader failure after '{plugin_id}' succeeded at source time {source_time} for {path:?}"
            ),
        )
    }
}

impl PluginManager {
    pub(crate) fn fail_effect_once_after_success(
        &self,
        component_id: &str,
        local_time: f64,
    ) -> Result<(), LibraryError> {
        self.render_failure.arm(FailureTarget::Effect {
            component_id: component_id.to_string(),
            local_time_bits: local_time.to_bits(),
            mode: FailureMode::Error,
        })
    }

    pub(crate) fn fail_video_loader_once_after_success(
        &self,
        plugin_id: &str,
        path: &Path,
        source_time: f64,
    ) -> Result<(), LibraryError> {
        self.arm_video_loader_failure(plugin_id, path, source_time, FailureMode::Error)
    }

    pub(crate) fn panic_video_loader_once_after_success(
        &self,
        plugin_id: &str,
        path: &Path,
        source_time: f64,
    ) -> Result<(), LibraryError> {
        self.arm_video_loader_failure(plugin_id, path, source_time, FailureMode::Panic)
    }

    fn arm_video_loader_failure(
        &self,
        plugin_id: &str,
        path: &Path,
        source_time: f64,
        mode: FailureMode,
    ) -> Result<(), LibraryError> {
        self.render_failure.arm(FailureTarget::VideoLoader {
            plugin_id: plugin_id.to_string(),
            path: path.to_string_lossy().into_owned(),
            source_time_bits: source_time.to_bits(),
            mode,
        })
    }
}

fn fail(mode: FailureMode, message: String) -> Result<(), LibraryError> {
    match mode {
        FailureMode::Error => Err(LibraryError::Plugin(message)),
        FailureMode::Panic => std::panic::panic_any(message),
    }
}
