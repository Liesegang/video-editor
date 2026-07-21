use egui::{Color32, RichText, Ui};
use egui_phosphor::regular as icons;
use library::model::project::PortOwner;
use library::model::property::{Property, PropertyValue};
use library::model::Project;
use library::plugin::{
    EvaluationContext, PluginManager, PropertyEvaluationError, PropertyEvaluationOutcome,
};
use uuid::Uuid;

use super::port_owner_composition;

pub(super) struct NodePropertyEvaluation {
    value: Option<PropertyValue>,
    issue: Option<NodePropertyIssue>,
}

impl NodePropertyEvaluation {
    pub(super) fn value(&self) -> Option<&PropertyValue> {
        self.value.as_ref()
    }

    pub(super) fn issue(&self) -> Option<&NodePropertyIssue> {
        self.issue.as_ref()
    }
}

pub(super) struct NodePropertyIssue {
    evaluator: String,
    source: Option<String>,
    message: String,
    recovered: bool,
}

pub(super) fn evaluate_node_property(
    project: &Project,
    plugin_manager: Option<&PluginManager>,
    node_id: Uuid,
    property: &Property,
    time: f64,
) -> NodePropertyEvaluation {
    let result = evaluate_with_context(project, plugin_manager, node_id, property, time);
    match result {
        Ok(outcome) => {
            let issue = outcome.diagnostic().map(|diagnostic| NodePropertyIssue {
                evaluator: diagnostic.evaluator().to_string(),
                source: property.expression_text().map(str::to_string),
                message: diagnostic.message().to_string(),
                recovered: true,
            });
            NodePropertyEvaluation {
                value: Some(outcome.into_value()),
                issue,
            }
        }
        Err(error) => NodePropertyEvaluation {
            value: None,
            issue: Some(NodePropertyIssue {
                evaluator: error.evaluator().to_string(),
                source: property.expression_text().map(str::to_string),
                message: error.message().to_string(),
                recovered: false,
            }),
        },
    }
}

fn evaluate_with_context(
    project: &Project,
    plugin_manager: Option<&PluginManager>,
    node_id: Uuid,
    property: &Property,
    time: f64,
) -> Result<PropertyEvaluationOutcome, PropertyEvaluationError> {
    let Some(plugin_manager) = plugin_manager else {
        return property
            .evaluate_at(time)
            .map(PropertyEvaluationOutcome::clean)
            .map_err(|error| {
                PropertyEvaluationError::new(error.evaluator(), error.message().to_string())
            });
    };
    let node = project.get_node(node_id).ok_or_else(|| {
        PropertyEvaluationError::new(property.evaluator.clone(), "Node is missing from Project")
    })?;
    let composition_id =
        port_owner_composition(project, PortOwner::Node(node_id)).ok_or_else(|| {
            PropertyEvaluationError::new(
                property.evaluator.clone(),
                "Node has no owning Composition evaluation context",
            )
        })?;
    let composition = project.get_composition(composition_id).ok_or_else(|| {
        PropertyEvaluationError::new(
            property.evaluator.clone(),
            "owning Composition is missing from Project",
        )
    })?;
    let context = EvaluationContext::new(
        node.properties(),
        composition.fps,
        (composition.width, composition.height),
    );
    plugin_manager
        .get_property_evaluators()
        .evaluate_with_diagnostics(property, time, &context)
}

pub(super) fn render_node_property_issue(
    ui: &mut Ui,
    node_id: Uuid,
    property: &str,
    issue: &NodePropertyIssue,
) {
    let color = if issue.recovered {
        Color32::from_rgb(235, 178, 70)
    } else {
        Color32::from_rgb(235, 95, 95)
    };
    let marker = if issue.recovered {
        icons::WARNING
    } else {
        icons::X_CIRCLE
    };
    let response = ui
        .add(
            egui::Label::new(RichText::new(marker).color(color).strong())
                .selectable(false)
                .sense(egui::Sense::hover()),
        )
        .on_hover_ui(|ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("node:{node_id}.{property} · {}", issue.evaluator))
                        .color(color)
                        .strong(),
                )
                .selectable(false),
            );
            ui.add(egui::Label::new(&issue.message).selectable(false).wrap());
            if let Some(source) = &issue.source {
                ui.add(
                    egui::Label::new(RichText::new(format!("source: {source}")).monospace())
                        .selectable(false)
                        .wrap(),
                );
            }
        });
    crate::qa::register_component_with_metadata(
        format!("node_editor.property_diagnostic:{node_id}:{property}"),
        "node_editor_property_diagnostic",
        response.rect,
        true,
        Some(serde_json::json!({
            "scope": format!("node:{node_id}"),
            "property": property,
            "evaluator": issue.evaluator,
            "source": issue.source,
            "message": issue.message,
            "recovered": issue.recovered,
        })),
    );
}

#[cfg(test)]
mod tests {
    use library::model::property::PropertyValue;
    use library::model::{Composition, Node, NodeContainer, Project};
    use ordered_float::OrderedFloat;

    use super::*;

    #[test]
    fn node_property_evaluation_uses_local_time_and_composition_metadata() {
        let mut project = Project::new("node expression");
        let (composition, track) = Composition::new("main", 100, 50, 24.0, 10.0);
        let composition_id = composition.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let node = Node::new_fmod("value");
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
            .unwrap();
        let property = Property::expression(
            "value + time + fps + width / 100".to_string(),
            PropertyValue::Number(OrderedFloat(0.5)),
        );
        let plugins = PluginManager::default();

        let evaluated = evaluate_node_property(&project, Some(&plugins), node_id, &property, 2.0);
        assert_eq!(
            evaluated.value(),
            Some(&PropertyValue::Number(OrderedFloat(27.5)))
        );
        assert!(evaluated.issue().is_none());

        let recovered = evaluate_node_property(
            &project,
            Some(&plugins),
            node_id,
            &Property::expression(
                "1 / 0".to_string(),
                PropertyValue::Number(OrderedFloat(0.25)),
            ),
            2.0,
        );
        assert_eq!(
            recovered.value(),
            Some(&PropertyValue::Number(OrderedFloat(0.25)))
        );
        assert!(recovered.issue().is_some_and(|issue| issue.recovered));
    }
}
