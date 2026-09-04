use std::sync::Arc;

use log::debug;

use crate::error::LibraryError;
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::project::{Composition, Project};
use crate::plugin::{PluginManager, PropertyEvaluatorRegistry};
use crate::util::timing::ScopedTimer;

mod color_graph;
mod container_graph;
mod data_graph;
mod evaluator;
mod image_graph;
mod input_preview;
mod list_graph;
mod path_graph;
mod scope;
mod shape_graph;
mod value_graph;

pub use evaluator::FrameEvaluator;
pub use input_preview::InputValuePreview;

pub fn evaluate_composition_frame(
    project: &Project,
    composition: &Composition,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    plugin_manager: &Arc<PluginManager>,
) -> Result<FrameInfo, LibraryError> {
    FrameEvaluator::new(
        project,
        composition,
        Arc::clone(property_evaluators),
        plugin_manager.as_ref(),
    )
    .evaluate(frame_number, render_scale, region)
}

pub fn get_frame_from_project(
    project: &Project,
    composition_index: usize,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    plugin_manager: &Arc<PluginManager>,
) -> Result<FrameInfo, LibraryError> {
    let composition = project
        .compositions
        .get(composition_index)
        .ok_or(LibraryError::InvalidCompositionIndex(composition_index))?;
    let _timer = log::log_enabled!(log::Level::Debug).then(|| {
        ScopedTimer::debug(format!(
            "Frame assembly comp={composition_index} frame={frame_number}"
        ))
    });
    let frame = evaluate_composition_frame(
        project,
        composition,
        frame_number,
        render_scale,
        region,
        property_evaluators,
        plugin_manager,
    )?;
    debug!(
        "Frame {frame_number} summary: objects={}",
        frame.object_count()
    );
    Ok(frame)
}

#[cfg(test)]
mod color_graph_tests;
#[cfg(test)]
mod data_graph_tests;
#[cfg(test)]
mod list_graph_tests;
#[cfg(test)]
mod path_graph_tests;
#[cfg(test)]
mod sound_analysis_tests;
#[cfg(test)]
mod tests;
