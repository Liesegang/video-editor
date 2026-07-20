use std::collections::HashMap;

use egui::{Color32, RichText, Ui};
use library::model::property::{PropertyMap, PropertyValue};
use library::EditorService;

pub(super) struct EvaluatedPropertyMap {
    values: HashMap<String, PropertyValue>,
    issues: Vec<PropertyEvaluationIssue>,
}

impl EvaluatedPropertyMap {
    pub(super) fn value(&self, name: &str) -> Option<&PropertyValue> {
        self.values.get(name)
    }

    pub(super) fn issues(&self) -> &[PropertyEvaluationIssue] {
        &self.issues
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PropertyEvaluationIssue {
    property: String,
    evaluator: String,
    source: Option<String>,
    message: String,
    recovered: bool,
}

impl PropertyEvaluationIssue {
    fn metadata(&self, scope: &str) -> serde_json::Value {
        serde_json::json!({
            "scope": scope,
            "property": self.property,
            "evaluator": self.evaluator,
            "source": self.source,
            "message": self.message,
            "recovered": self.recovered,
        })
    }
}

pub(super) fn evaluate_property_map(
    project_service: &EditorService,
    properties: &PropertyMap,
    time: f64,
    fps: f64,
    resolution: (u64, u64),
) -> EvaluatedPropertyMap {
    let mut values = HashMap::new();
    let mut issues = Vec::new();
    let mut entries = properties.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(name, _)| name.as_str());

    for (name, property) in entries {
        match project_service
            .evaluate_property_with_diagnostics(property, properties, time, fps, resolution)
        {
            Ok(outcome) => {
                values.insert(name.clone(), outcome.value().clone());
                if let Some(diagnostic) = outcome.diagnostic() {
                    issues.push(PropertyEvaluationIssue {
                        property: name.clone(),
                        evaluator: diagnostic.evaluator().to_string(),
                        source: property.expression_text().map(str::to_string),
                        message: diagnostic.message().to_string(),
                        recovered: true,
                    });
                }
            }
            Err(error) => issues.push(PropertyEvaluationIssue {
                property: name.clone(),
                evaluator: error.evaluator().to_string(),
                source: property.expression_text().map(str::to_string),
                message: error.message().to_string(),
                recovered: false,
            }),
        }
    }

    EvaluatedPropertyMap { values, issues }
}

pub(super) fn render_evaluation_issues(
    ui: &mut Ui,
    scope: &str,
    issues: &[PropertyEvaluationIssue],
) {
    for issue in issues {
        let color = if issue.recovered {
            Color32::from_rgb(235, 178, 70)
        } else {
            Color32::from_rgb(235, 95, 95)
        };
        let marker = if issue.recovered { "Warning" } else { "Error" };
        let response = egui::Frame::new()
            .fill(color.gamma_multiply(0.10))
            .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.65)))
            .corner_radius(4)
            .inner_margin(egui::Margin::same(5))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{marker} · {scope}.{} · {}",
                            issue.property, issue.evaluator
                        ))
                        .color(color)
                        .strong(),
                    )
                    .selectable(false)
                    .wrap(),
                );
                ui.add(
                    egui::Label::new(RichText::new(&issue.message).small())
                        .selectable(false)
                        .wrap(),
                );
                if let Some(source) = &issue.source {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("source: {source}"))
                                .small()
                                .monospace(),
                        )
                        .selectable(false)
                        .wrap(),
                    );
                }
            })
            .response;

        crate::qa::register_component_with_metadata(
            format!("inspector.property_diagnostic.{scope}:{}", issue.property),
            "inspector_property_diagnostic",
            response.rect,
            true,
            Some(issue.metadata(scope)),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use library::cache::CacheManager;
    use library::model::property::Property;
    use library::model::Project;
    use library::plugin::PluginManager;
    use ordered_float::OrderedFloat;

    use super::*;

    #[test]
    fn diagnostic_metadata_keeps_scope_property_source_and_recovery_state() {
        let issue = PropertyEvaluationIssue {
            property: "opacity".to_string(),
            evaluator: "expression".to_string(),
            source: Some("1 / 0".to_string()),
            message: "division by zero".to_string(),
            recovered: true,
        };
        assert_eq!(
            issue.metadata("node:abc"),
            serde_json::json!({
                "scope": "node:abc",
                "property": "opacity",
                "evaluator": "expression",
                "source": "1 / 0",
                "message": "division by zero",
                "recovered": true,
            })
        );
    }

    #[test]
    fn expression_fallback_remains_editable_while_malformed_property_is_blocked() {
        let service = EditorService::new(
            Arc::new(RwLock::new(Project::new("expression inspector"))),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )
        .unwrap();
        let mut properties = PropertyMap::new();
        properties.set(
            "opacity".to_string(),
            Property::expression(
                "1 / 0".to_string(),
                PropertyValue::Number(OrderedFloat(0.75)),
            ),
        );
        properties.set(
            "malformed".to_string(),
            Property {
                evaluator: "expression".to_string(),
                properties: HashMap::from([(
                    "expression".to_string(),
                    PropertyValue::String("1".to_string()),
                )]),
            },
        );

        let evaluated = evaluate_property_map(&service, &properties, 2.0, 24.0, (100, 50));
        assert_eq!(
            evaluated.value("opacity"),
            Some(&PropertyValue::Number(OrderedFloat(0.75)))
        );
        assert!(evaluated.value("malformed").is_none());
        assert_eq!(evaluated.issues().len(), 2);
        assert!(evaluated.issues().iter().any(|issue| {
            issue.property == "opacity"
                && issue.recovered
                && issue.source.as_deref() == Some("1 / 0")
        }));
        assert!(evaluated
            .issues()
            .iter()
            .any(|issue| issue.property == "malformed" && !issue.recovered));
    }
}
