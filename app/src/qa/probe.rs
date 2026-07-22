use super::server::HttpResponse;
use super::ui_query::{self, UiQuery, UiQueryKind};
use library::core::framing::FrameEvaluator;
use library::model::project::{EvalOutput, PortAddress, PortOwner, Project};
use library::plugin::PluginManager;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::mpsc::SyncSender;
use uuid::Uuid;

const MAX_PORT_BYTES: usize = 128;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MetadataOutputProbeRequest {
    pub node_id: Uuid,
    pub port: String,
    pub global_time: f64,
}

impl MetadataOutputProbeRequest {
    fn validate(&self) -> Result<(), String> {
        if self.port.trim().is_empty() {
            return Err("port must not be empty".to_string());
        }
        if self.port.len() > MAX_PORT_BYTES {
            return Err(format!("port must not exceed {MAX_PORT_BYTES} bytes"));
        }
        if !self.global_time.is_finite() {
            return Err("global_time must be finite".to_string());
        }
        Ok(())
    }
}

pub(super) fn endpoint_response(
    body: &[u8],
    sender: &SyncSender<UiQuery>,
    repaint_context: &egui::Context,
) -> HttpResponse {
    let request = match serde_json::from_slice::<MetadataOutputProbeRequest>(body) {
        Ok(request) => request,
        Err(error) => {
            return HttpResponse::json(
                400,
                json!({"error": format!("invalid JSON body: {error}")}),
            );
        }
    };
    if let Err(error) = request.validate() {
        return HttpResponse::json(422, json!({"error": error}));
    }
    ui_query::query_response(
        UiQueryKind::MetadataOutput(request),
        sender,
        repaint_context,
        "metadata output",
    )
}

pub fn evaluate_metadata_output(
    project: &Project,
    active_composition_id: Option<Uuid>,
    request: &MetadataOutputProbeRequest,
    plugin_manager: &PluginManager,
) -> Result<Value, String> {
    let composition_id = project
        .find_containing_composition(request.node_id)
        .or(active_composition_id)
        .ok_or_else(|| "Node has no containing or active composition".to_string())?;
    let composition = project
        .get_composition(composition_id)
        .ok_or_else(|| "Node composition is absent".to_string())?;
    let evaluator = FrameEvaluator::new(
        project,
        composition,
        plugin_manager.get_property_evaluators(),
        plugin_manager,
    );
    let source = PortAddress::new(PortOwner::Node(request.node_id), request.port.clone());
    let output = match evaluator.evaluate_metadata_output(&source, request.global_time) {
        Ok(EvalOutput::Produced(value)) => json!({
            "status": "produced",
            "value": Value::from(&value),
        }),
        Ok(EvalOutput::NoOutput) => json!({"status": "no_output"}),
        Err(error) => json!({
            "status": "error",
            "error": error.to_string(),
        }),
    };
    Ok(json!({
        "node_id": request.node_id,
        "port": request.port.as_str(),
        "global_time": request.global_time,
        "evaluation_source": "authoritative_project",
        "result": output,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::HistoryManager;
    use crate::state::context::EditorContext;
    use crate::ui::tab_viewer::create_initial_dock_state;
    use library::model::project::connection::{DATA_VALUE_OUTPUT_PORT, DATA_VALUE_PROPERTY};
    use library::model::project::{Composition, NodeContainer};
    use library::model::property::{ColorSpaceRef, ColorValue, Property, PropertyValue};
    use library::model::{DataContent, Node};
    use library::plugin::{
        EvaluationContext, Plugin, PropertyEvaluationError, PropertyEvaluator, PropertyPlugin,
    };
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingEvaluator {
        evaluations: Arc<AtomicUsize>,
        output: PropertyValue,
    }

    impl PropertyEvaluator for CountingEvaluator {
        fn evaluate(
            &self,
            _property: &Property,
            _time: f64,
            _context: &EvaluationContext,
        ) -> Result<PropertyValue, PropertyEvaluationError> {
            self.evaluations.fetch_add(1, Ordering::SeqCst);
            Ok(self.output.clone())
        }
    }

    struct CountingPlugin {
        evaluations: Arc<AtomicUsize>,
        output: PropertyValue,
    }

    impl Plugin for CountingPlugin {
        fn id(&self) -> &str {
            "qa-counting"
        }

        fn name(&self) -> String {
            "QA Counting".to_string()
        }

        fn category(&self) -> String {
            "Tests".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl PropertyPlugin for CountingPlugin {
        fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
            Arc::new(CountingEvaluator {
                evaluations: Arc::clone(&self.evaluations),
                output: self.output.clone(),
            })
        }
    }

    #[test]
    fn snapshot_evaluates_nothing_and_explicit_probe_evaluates_once() -> Result<(), String> {
        let color_space =
            ColorSpaceRef::new("scene_linear_ap1").map_err(|error| error.to_string())?;
        let expected = PropertyValue::ColorValue(
            ColorValue::new(color_space, [0.2, 3.0, 0.4, 0.5])
                .map_err(|error| error.to_string())?,
        );
        let evaluations = Arc::new(AtomicUsize::new(0));
        let plugin_manager = PluginManager::default();
        plugin_manager.register_property_plugin(Arc::new(CountingPlugin {
            evaluations: Arc::clone(&evaluations),
            output: expected.clone(),
        }));

        let mut project = Project::new("QA probe");
        let (composition, track) = Composition::new("Main", 320, 180, 30.0, 2.0);
        let composition_id = composition.id;
        project
            .add_track(track)
            .map_err(|error| error.to_string())?;
        project
            .add_composition(composition)
            .map_err(|error| error.to_string())?;
        let mut data_node = Node::new_data("Color", DataContent::Color);
        data_node.set_property(
            DATA_VALUE_PROPERTY.to_string(),
            Property {
                evaluator: "qa-counting".to_string(),
                properties: HashMap::from([("value".to_string(), expected.clone())]),
            },
        )?;
        let node_id = data_node.id;
        project.add_node(data_node);
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
            .map_err(|error| error.to_string())?;
        let before = project.clone();
        let context = EditorContext::new(composition_id);

        let snapshot = crate::qa::state::snapshot(
            7,
            &project,
            &context,
            &create_initial_dock_state(),
            &HistoryManager::new(),
        )?;
        assert_eq!(evaluations.load(Ordering::SeqCst), 0);
        assert!(snapshot.get("runtime").is_none());

        let request = MetadataOutputProbeRequest {
            node_id,
            port: DATA_VALUE_OUTPUT_PORT.to_string(),
            global_time: 0.75,
        };
        let runtime =
            evaluate_metadata_output(&project, Some(composition_id), &request, &plugin_manager)?;
        assert_eq!(evaluations.load(Ordering::SeqCst), 1);
        assert_eq!(runtime["result"]["status"], "produced");
        assert_eq!(runtime["result"]["value"], Value::from(&expected));
        assert_eq!(project, before, "QA probe must not mutate the Project");
        Ok(())
    }

    #[test]
    fn probe_request_validation_is_bounded() {
        assert!(MetadataOutputProbeRequest {
            node_id: Uuid::new_v4(),
            port: " ".to_string(),
            global_time: 0.0,
        }
        .validate()
        .is_err());
        assert!(MetadataOutputProbeRequest {
            node_id: Uuid::new_v4(),
            port: "x".repeat(MAX_PORT_BYTES + 1),
            global_time: 0.0,
        }
        .validate()
        .is_err());
        assert!(MetadataOutputProbeRequest {
            node_id: Uuid::new_v4(),
            port: "value".to_string(),
            global_time: f64::INFINITY,
        }
        .validate()
        .is_err());
    }
}
