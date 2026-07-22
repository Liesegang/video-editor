//! Evaluation of typed metadata and numeric value connections.
//!
//! This module owns connected property inputs, bypass routing, and arithmetic
//! Nodes. Timeline inheritance and local-time derivation remain in `scope`.

use std::collections::HashSet;
use std::sync::LazyLock;

use uuid::Uuid;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error};
use super::scope::EvaluationScope;
use crate::cache::CacheManager;
use crate::core::audio::analysis::{Spectrum, band_energy, peak, rms, spectrum};
use crate::core::audio::mixer::render_owner_samples;
use crate::error::LibraryError;
use crate::model::numeric::evaluate_numeric_binary;
use crate::model::project::{
    ANALYSIS_HOP_MS_PROPERTY, ANALYSIS_SAMPLE_RATE_PROPERTY, ANALYSIS_WINDOW_MS_PROPERTY,
    BAND_HIGH_HZ_PROPERTY, BAND_LOW_HZ_PROPERTY, DURATION_PORT, EvalOutput, EvalResult,
    NUMBER_RESULT_OUTPUT_PORT, PortAddress, PortDataType, PortDirection, PortOwner,
    RESOLUTION_PORT, SOUND_INPUT_PORT, SPECTRUM_INPUT_PORT, SPECTRUM_OUTPUT_PORT, TIME_PORT,
};
use crate::model::property::PropertyValue;
use crate::model::{Node, NodeContent, SoundAnalysisContent, ValueContent};
use crate::plugin::{PropertyEvaluationError, ResolvedNodeInputs, property_name_from_port};

static SOUND_ANALYSIS_CACHE: LazyLock<CacheManager> = LazyLock::new(CacheManager::new);
// Matches the authoritative hard limits: 2,000 ms at 192 kHz. Spectrum pads
// this to 524,288 FFT points (about 8 MiB for Complex bins plus magnitudes),
// which remains bounded without silently shortening a legal authored window.
const MAX_SOUND_ANALYSIS_WINDOW_SAMPLES: usize = 384_000;

impl FrameEvaluator<'_> {
    pub(super) fn resolve_node_inputs(
        &self,
        node_id: Uuid,
        scope: EvaluationScope,
        global_time: f64,
    ) -> Result<ResolvedNodeInputs, LibraryError> {
        let mut values = ResolvedNodeInputs::from_metadata(scope.as_inputs());
        let owner = PortOwner::Node(node_id);
        let targets = self
            .project
            .connections
            .iter()
            .filter(|connection| connection.to.owner == owner)
            .map(|connection| connection.to.clone())
            .collect::<HashSet<_>>();
        for target in targets {
            let target_definition = self
                .project
                .port_definition(&target, PortDirection::Input)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Missing input port {target:?}"))
                })?;
            match target_definition.data_type {
                PortDataType::Image
                | PortDataType::Shape
                | PortDataType::Audio
                | PortDataType::Spectrum => continue,
                _ => {}
            }
            if matches!(
                target.port.as_str(),
                TIME_PORT | DURATION_PORT | RESOLUTION_PORT
            ) {
                // Authored scope overrides have already been applied by
                // scope_for_owner. Keeping a second copy in the property map
                // both re-evaluates the graph and obscures which Time is
                // authoritative.
                continue;
            }
            let connection = match self.single_connection_to(&target)? {
                EvalOutput::Produced(connection) => connection,
                EvalOutput::NoOutput => continue,
            };
            let value =
                self.resolve_metadata_value(&connection.from, global_time, &mut HashSet::new())?;
            let logical_key = property_name_from_port(&target.port).unwrap_or(&target.port);
            values.properties.insert(logical_key.to_string(), value);
        }
        Ok(values)
    }

    pub(super) fn resolve_metadata_value(
        &self,
        source: &PortAddress,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let source_node = if let PortOwner::Node(node_id) = source.owner {
            let node = self
                .project
                .get_node(node_id)
                .ok_or_else(|| missing_error(source.owner))?;
            if !node.enabled {
                return Ok(EvalOutput::NoOutput);
            }
            Some(node)
        } else {
            None
        };
        let definition = self
            .project
            .port_definition(source, PortDirection::Output)
            .ok_or_else(|| LibraryError::Validation(format!("Missing output port {source:?}")))?;
        if matches!(
            definition.data_type,
            PortDataType::Image | PortDataType::Audio | PortDataType::Spectrum
        ) {
            return Err(LibraryError::Validation(format!(
                "Typed media port {source:?} cannot be resolved as a value"
            )));
        }
        if let Some(node) = source_node
            && node.bypassed
        {
            if !path.insert(source.owner) {
                return Err(cycle_error(source.owner));
            }
            let result = (|| {
                let Some(input) = node.bypass_input_for_output(&source.port) else {
                    log::warn!(
                        "Node {} has an invalid bypass flag for output {:?}; producing NoOutput",
                        node.id,
                        source.port
                    );
                    return Ok(EvalOutput::NoOutput);
                };
                let target = PortAddress::new(source.owner, input);
                let connection = match self.single_connection_to(&target)? {
                    EvalOutput::Produced(connection) => connection,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                self.resolve_metadata_value(&connection.from, global_time, path)
            })();
            path.remove(&source.owner);
            return result;
        }
        if let Some(NodeContent::CompositionInstance(instance)) = source_node.map(Node::content) {
            return match self.composition_instance_target_scope(
                source.owner.id(),
                instance,
                global_time,
                path,
            )? {
                EvalOutput::Produced(scope) => scope
                    .value(&source.port)
                    .map(EvalOutput::Produced)
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Unsupported Composition Instance metadata output {source:?}"
                        ))
                    }),
                EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
            };
        }
        if let PortOwner::Node(node_id) = source.owner
            && matches!(source_node.map(Node::content), Some(NodeContent::Value(_)))
        {
            return self.evaluate_value_node_output(node_id, &source.port, global_time, path);
        }
        if let PortOwner::Node(node_id) = source.owner
            && matches!(source_node.map(Node::content), Some(NodeContent::List(_)))
        {
            return self.evaluate_list_node_output(node_id, &source.port, global_time, path);
        }
        if let PortOwner::Node(node_id) = source.owner
            && matches!(source_node.map(Node::content), Some(NodeContent::Color(_)))
        {
            return self.evaluate_color_node_output(node_id, &source.port, global_time, path);
        }
        if let PortOwner::Node(node_id) = source.owner
            && matches!(source_node.map(Node::content), Some(NodeContent::Data(_)))
        {
            return self.evaluate_data_node_output(node_id, &source.port, global_time, path);
        }
        if let PortOwner::Node(node_id) = source.owner
            && let Some(NodeContent::Path(operation)) = source_node.map(Node::content)
        {
            return self.evaluate_path_operation_output(
                node_id,
                *operation,
                &source.port,
                global_time,
                path,
            );
        }
        if let PortOwner::Node(node_id) = source.owner
            && matches!(
                source_node.map(Node::content),
                Some(NodeContent::SoundAnalysis(_))
            )
        {
            return self.evaluate_sound_analysis_value_output(
                node_id,
                &source.port,
                global_time,
                path,
            );
        }
        if let Some(NodeContent::PluginOperation(operation)) = source_node.map(Node::content) {
            let descriptor = match self.plugin_manager.operation_descriptor(
                &operation.category,
                &operation.component_id,
                &operation.operation,
            ) {
                Ok(descriptor) => descriptor,
                Err(_) => return Ok(EvalOutput::NoOutput),
            };
            if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
                return Ok(EvalOutput::NoOutput);
            }
        }
        if let Some(NodeContent::NativeOperation(operation)) = source_node.map(Node::content) {
            let diagnostic = crate::model::native_node_descriptor(&operation.catalog_id)
                .and_then(|descriptor| descriptor.runtime_diagnostic())
                .unwrap_or_else(|| {
                    format!(
                        "Unknown native catalog id '{}'; evaluation produces No Output",
                        operation.catalog_id
                    )
                });
            log::warn!("Native catalog node {}: {diagnostic}", source.owner.id());
            return Ok(EvalOutput::NoOutput);
        }
        match self.scope_for_owner(source.owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope
                .value(&source.port)
                .map(EvalOutput::Produced)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Unsupported value output port {source:?}"))
                }),
            EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
        }
    }

    fn evaluate_value_node_output(
        &self,
        node_id: Uuid,
        output_port: &str,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        if !node.enabled {
            return Ok(EvalOutput::NoOutput);
        }
        if output_port != crate::model::project::NUMBER_RESULT_OUTPUT_PORT {
            return Err(LibraryError::Validation(format!(
                "Unsupported value output port {owner:?}.{output_port}"
            )));
        }
        let scope = match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = match node.content() {
            NodeContent::Value(value) => {
                self.evaluate_numeric_binary_node(node, *value, scope, global_time, path)
            }
            _ => Ok(EvalOutput::NoOutput),
        };
        path.remove(&owner);
        result
    }

    fn evaluate_numeric_binary_node(
        &self,
        node: &Node,
        value: ValueContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let left = match self.resolve_value_input(
            node,
            value.primary_input(),
            None,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let right = match self.resolve_value_input(
            node,
            value.secondary_input(),
            Some(value.secondary_input()),
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        Ok(
            evaluate_numeric_binary(value.numeric_operation(), &left, &right)
                .map_or(EvalOutput::NoOutput, EvalOutput::Produced),
        )
    }

    fn resolve_value_input(
        &self,
        node: &Node,
        port: &str,
        property_fallback: Option<&str>,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let target = PortAddress::new(PortOwner::Node(node.id), port);
        match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => {
                self.resolve_metadata_value(&connection.from, global_time, path)
            }
            EvalOutput::NoOutput => {
                let Some(property_key) = property_fallback else {
                    return Ok(EvalOutput::NoOutput);
                };
                let Some(property) = node.properties().get(property_key) else {
                    return Ok(EvalOutput::NoOutput);
                };
                let composition = self
                    .composition_for_owner(PortOwner::Node(node.id))
                    .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
                let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
                let context = self.context(composition, Some(&inputs));
                let properties = node.properties();
                let value = context.evaluate_property_value(property, properties, scope.time);
                Ok(property_output(value, node.id, property_key))
            }
        }
    }

    fn evaluate_sound_analysis_value_output(
        &self,
        node_id: Uuid,
        output_port: &str,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        let NodeContent::SoundAnalysis(analysis) = node.content() else {
            return Ok(EvalOutput::NoOutput);
        };
        if !node.enabled || output_port != NUMBER_RESULT_OUTPUT_PORT {
            return Ok(EvalOutput::NoOutput);
        }
        let scope = match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = match analysis {
            SoundAnalysisContent::Rms | SoundAnalysisContent::Peak => {
                self.evaluate_sound_scalar(node, *analysis, scope, global_time, path)
            }
            SoundAnalysisContent::BandEnergy => {
                self.evaluate_band_energy(node, scope, global_time, path)
            }
            SoundAnalysisContent::Spectrum => Ok(EvalOutput::NoOutput),
        };
        path.remove(&owner);
        result.map(|output| output.map(PropertyValue::from))
    }

    fn evaluate_sound_scalar(
        &self,
        node: &Node,
        analysis: SoundAnalysisContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<f64> {
        let samples = match self.resolve_sound_window(node, scope, global_time, path)? {
            EvalOutput::Produced(samples) => samples,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        Ok(EvalOutput::Produced(match analysis {
            SoundAnalysisContent::Rms => rms(&samples),
            SoundAnalysisContent::Peak => peak(&samples),
            SoundAnalysisContent::Spectrum | SoundAnalysisContent::BandEnergy => {
                return Ok(EvalOutput::NoOutput);
            }
        }))
    }

    fn evaluate_band_energy(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<f64> {
        let target = PortAddress::new(PortOwner::Node(node.id), SPECTRUM_INPUT_PORT);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let spectrum = match self.resolve_spectrum(&connection.from, global_time, path)? {
            EvalOutput::Produced(spectrum) => spectrum,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let low = match self.evaluate_analysis_property(
            node,
            BAND_LOW_HZ_PROPERTY,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let high = match self.evaluate_analysis_property(
            node,
            BAND_HIGH_HZ_PROPERTY,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if high < low {
            return Ok(EvalOutput::NoOutput);
        }
        Ok(EvalOutput::Produced(band_energy(&spectrum, low, high)))
    }

    fn resolve_spectrum(
        &self,
        source: &PortAddress,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Spectrum> {
        if source.port != SPECTRUM_OUTPUT_PORT {
            return Ok(EvalOutput::NoOutput);
        }
        let PortOwner::Node(node_id) = source.owner else {
            return Ok(EvalOutput::NoOutput);
        };
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(source.owner))?;
        if !node.enabled
            || !matches!(
                node.content(),
                NodeContent::SoundAnalysis(SoundAnalysisContent::Spectrum)
            )
        {
            return Ok(EvalOutput::NoOutput);
        }
        let scope = match self.scope_for_owner(source.owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(source.owner) {
            return Err(cycle_error(source.owner));
        }
        let result = (|| {
            let samples = match self.resolve_sound_window(node, scope, global_time, path)? {
                EvalOutput::Produced(samples) => samples,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
            let sample_rate = match self.analysis_sample_rate(node, scope, global_time, path)? {
                EvalOutput::Produced(sample_rate) => sample_rate,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
            Ok(EvalOutput::Produced(spectrum(&samples, sample_rate)))
        })();
        path.remove(&source.owner);
        result
    }

    fn resolve_sound_window(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Vec<f32>> {
        // TODO(perf/audio-analysis-fanout): add FrameEvaluator-request-local
        // memoization for mixed PCM windows and FFT results. The shared audio
        // chunk cache already prevents repeated file decode, but sibling
        // RMS/Peak/Spectrum branches still remix the same window and multiple
        // Band Energy consumers recompute the same FFT. Benchmark a fan-out
        // graph before choosing the cache key and memory bound.
        let target = PortAddress::new(PortOwner::Node(node.id), SOUND_INPUT_PORT);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let sample_rate = match self.analysis_sample_rate(node, scope, global_time, path)? {
            EvalOutput::Produced(sample_rate) => sample_rate,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let window_ms = match self.evaluate_analysis_property(
            node,
            ANALYSIS_WINDOW_MS_PROPERTY,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) if value.is_finite() && value > 0.0 => value,
            EvalOutput::Produced(_) | EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let hop_ms = match self.evaluate_analysis_property(
            node,
            ANALYSIS_HOP_MS_PROPERTY,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) if value.is_finite() && value > 0.0 => value,
            EvalOutput::Produced(_) | EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let window_seconds = window_ms / 1_000.0;
        let hop_seconds = hop_ms / 1_000.0;
        // Analysis windows are quantized in the current Composition's output
        // timeline. Each sample is then routed through Clip stretch/trim and
        // explicit Time wires by AudioGraphEvaluator; quantizing local scope
        // here would apply those remaps twice.
        let center_time = (global_time / hop_seconds).floor() * hop_seconds;
        let start_time = (center_time - window_seconds * 0.5).max(0.0);
        let Some(frames) = sound_analysis_window_frames(window_seconds, sample_rate) else {
            return Ok(EvalOutput::NoOutput);
        };
        let start_sample = (start_time * f64::from(sample_rate)).floor() as u64;
        let Some(composition) = self.composition_for_owner(PortOwner::Node(node.id)) else {
            return Ok(EvalOutput::NoOutput);
        };
        Ok(render_owner_samples(
            self.project,
            composition,
            &connection.from,
            &SOUND_ANALYSIS_CACHE,
            start_sample,
            frames,
            sample_rate,
            global_time,
            self.plugin_manager,
        )
        .map_or(EvalOutput::NoOutput, EvalOutput::Produced))
    }

    fn analysis_sample_rate(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<u32> {
        let value = match self.evaluate_analysis_property(
            node,
            ANALYSIS_SAMPLE_RATE_PROPERTY,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if value.is_finite() && (8_000.0..=192_000.0).contains(&value) {
            Ok(EvalOutput::Produced(value.round() as u32))
        } else {
            Ok(EvalOutput::NoOutput)
        }
    }

    fn evaluate_analysis_property(
        &self,
        node: &Node,
        key: &str,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<f64> {
        let target = PortAddress::new(PortOwner::Node(node.id), key);
        let value = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => {
                match self.resolve_metadata_value(&connection.from, global_time, path)? {
                    EvalOutput::Produced(value) => value,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                }
            }
            EvalOutput::NoOutput => {
                let Some(property) = node.properties().get(key) else {
                    return Ok(EvalOutput::NoOutput);
                };
                let composition = self
                    .composition_for_owner(PortOwner::Node(node.id))
                    .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
                let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
                let context = self.context(composition, Some(&inputs));
                context
                    .evaluate_property_value(property, node.properties(), scope.time)
                    .map_err(|error| {
                        LibraryError::Validation(format!(
                            "Sound analysis property '{}.{key}' failed: {error}",
                            node.id
                        ))
                    })?
            }
        };
        let numeric = match value {
            PropertyValue::Number(value) => value.into_inner(),
            PropertyValue::Integer(value) => value as f64,
            _ => return Ok(EvalOutput::NoOutput),
        };
        let NodeContent::SoundAnalysis(analysis) = node.content() else {
            return Ok(EvalOutput::NoOutput);
        };
        Ok(
            if analysis.numeric_property_is_in_hard_limits(key, numeric) {
                EvalOutput::Produced(numeric)
            } else {
                EvalOutput::NoOutput
            },
        )
    }
}

fn sound_analysis_window_frames(window_seconds: f64, sample_rate: u32) -> Option<usize> {
    let frames = (window_seconds * f64::from(sample_rate)).ceil();
    (frames.is_finite() && frames >= 1.0 && frames <= MAX_SOUND_ANALYSIS_WINDOW_SAMPLES as f64)
        .then_some(frames as usize)
}

fn property_output(
    result: Result<PropertyValue, PropertyEvaluationError>,
    node_id: Uuid,
    property_key: &str,
) -> EvalOutput<PropertyValue> {
    match result {
        Ok(value) => EvalOutput::Produced(value),
        Err(error) => {
            log::error!("Node '{node_id}' property '{property_key}' produced no output: {error}");
            EvalOutput::NoOutput
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SOUND_ANALYSIS_WINDOW_SAMPLES, sound_analysis_window_frames};

    #[test]
    fn legal_maximum_analysis_window_is_not_silently_shortened() {
        assert_eq!(
            sound_analysis_window_frames(2.0, 192_000),
            Some(MAX_SOUND_ANALYSIS_WINDOW_SAMPLES)
        );
        assert_eq!(
            MAX_SOUND_ANALYSIS_WINDOW_SAMPLES.next_power_of_two(),
            524_288
        );
        assert_eq!(sound_analysis_window_frames(2.0, 192_001), None);
    }
}
