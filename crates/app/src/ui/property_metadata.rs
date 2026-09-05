//! Shared typed property metadata for every editor surface that presents a
//! production Node. Node Editor and Inspector must resolve the same persisted
//! or canonical descriptor rather than maintaining panel-local copies.

use library::model::property::PropertyDefinition;
use library::model::{Node, NodeContent};
use library::plugin::PluginManager;

pub(crate) fn published_parameter_keyframe_capability(
    definition: &library::model::authoring::ModuleDefinition,
    parameter_id: library::model::authoring::PublishedParameterId,
) -> (bool, Option<&'static str>) {
    use library::model::authoring::PublishedParameterAutomationCapability;

    match definition.parameter_automation_capability(parameter_id) {
        Ok(PublishedParameterAutomationCapability::FrameSampled) => (true, None),
        Ok(PublishedParameterAutomationCapability::ConstantOnly { reason }) => {
            (false, Some(reason))
        }
        Err(_) => (
            false,
            Some("the Published Parameter target is invalid in this Module Definition"),
        ),
    }
}

pub(crate) fn node_property_definition(
    plugins: &PluginManager,
    node: &Node,
    property_name: &str,
) -> Option<PropertyDefinition> {
    let definitions: &[PropertyDefinition] = match node.content() {
        NodeContent::Value(value) => value.property_definitions(),
        NodeContent::Color(value) => value.property_definitions(),
        NodeContent::SoundAnalysis(value) => value.property_definitions(),
        NodeContent::Data(value) => value.property_definitions(),
        NodeContent::List(value) => value.property_definitions(),
        NodeContent::Path(value) => value.property_definitions(),
        NodeContent::PluginOperation(operation) => {
            let direct_ensemble_contract =
                library::model::authoring::text_ensemble_direct_contract_is_compatible(
                    &operation.declared_ports,
                ) && (operation.category == library::plugin::EFFECTOR_CATEGORY
                    || operation.category == library::plugin::DECORATOR_CATEGORY);
            let descriptor = if direct_ensemble_contract {
                plugins.text_ensemble_operation_descriptor(
                    &operation.category,
                    &operation.component_id,
                )
            } else {
                plugins.operation_descriptor(
                    &operation.category,
                    &operation.component_id,
                    &operation.operation,
                )
            };
            return descriptor
                .ok()?
                .properties()
                .iter()
                .find(|definition| definition.name() == property_name)
                .cloned();
        }
        NodeContent::NativeOperation(_) => {
            return library::model::native_node_descriptor_for_node(node)?
                .property_definitions()
                .into_iter()
                .find(|definition| definition.name() == property_name);
        }
        NodeContent::ModuleOutput(_)
        | NodeContent::Media(_)
        | NodeContent::Generator(_)
        | NodeContent::CompositionInstance(_)
        | NodeContent::Merge
        | NodeContent::SoundMerge => return None,
    };
    definitions
        .iter()
        .find(|definition| definition.name() == property_name)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_and_graph_text_operations_use_their_exact_contract_metadata() {
        let plugins = PluginManager::default();
        let inline = plugins
            .create_text_ensemble_operation_node(library::plugin::DECORATOR_CATEGORY, "backplate")
            .expect("inline Backplate node");
        for property in ["target", "shape", "color", "padding", "radius"] {
            assert!(
                node_property_definition(&plugins, &inline, property).is_some(),
                "inline Backplate is missing descriptor metadata for {property}"
            );
        }

        let graph = plugins
            .create_decorator_operation_node("backplate")
            .expect("graph Backplate node");
        assert!(node_property_definition(&plugins, &graph, "offset").is_some());
        assert!(node_property_definition(&plugins, &graph, "shape").is_none());
    }

    #[test]
    fn native_particle_property_uses_the_catalog_definition() {
        let plugins = PluginManager::default();
        let emitter = Node::new_catalog_node("native.particle.emitter").expect("Particle Emitter");
        let rate = node_property_definition(&plugins, &emitter, "rate")
            .expect("Particle Emitter rate definition");

        assert_eq!(rate.label(), "Rate");
        assert!(matches!(
            rate.ui_type(),
            library::model::property::PropertyUiType::Float {
                min: 0.0,
                max: 100_000.0,
                step: 0.1,
                min_hard_limit: true,
                max_hard_limit: true,
                ..
            }
        ));
        assert!(node_property_definition(&plugins, &emitter, "unknown").is_none());
    }
}
