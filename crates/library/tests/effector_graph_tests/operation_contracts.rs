use std::sync::{Arc, RwLock};

use anyhow::{Context, Result as AnyResult, bail};
use library::editor::project_service::ProjectManager;
use library::model::NodeContent;
use library::model::project::{
    NodeContainer, PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner, PortSide,
    Project, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::Property;
use library::plugin::{
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, PluginManager, property_port_key,
    property_ui_type_to_port_data_type,
};

use super::support::{HEIGHT, WIDTH, setup_project};

#[test]
fn descriptors_factories_and_text_shape_consumers_have_complete_typed_contracts() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let mut available = plugins.get_available_effectors();
    available.sort();
    assert_eq!(
        available,
        [
            "opacity",
            "randomize",
            "step_delay",
            "tracking",
            "transform"
        ]
    );

    for component_id in available {
        let descriptor = plugins
            .operation_descriptor(EFFECTOR_CATEGORY, &component_id, EFFECTOR_APPLY_OPERATION)
            .with_context(|| format!("missing descriptor for Effector component {component_id}"))?;
        let node = plugins
            .create_effector_operation_node(&component_id)
            .with_context(|| format!("create Effector operation {component_id}"))?;
        let NodeContent::PluginOperation(operation) = node.content() else {
            bail!("Effector factory must create a plugin operation");
        };
        assert_eq!(operation.category, EFFECTOR_CATEGORY);
        assert_eq!(operation.component_id, component_id);
        assert_eq!(operation.operation, EFFECTOR_APPLY_OPERATION);
        assert_eq!(operation.declared_ports, descriptor.declared_ports());
        for definition in descriptor.properties() {
            assert_eq!(
                node.properties()
                    .get(definition.name())
                    .and_then(Property::value),
                Some(definition.default_value())
            );
            let port = operation
                .declared_ports
                .iter()
                .find(|port| port.key == property_port_key(definition.name()))
                .with_context(|| format!("property {} has no port", definition.name()))?;
            assert_eq!(port.direction, PortDirection::Input);
            assert_eq!(
                port.data_type,
                property_ui_type_to_port_data_type(definition.ui_type())
            );
        }
        let input = operation
            .declared_ports
            .iter()
            .find(|port| port.key == SHAPE_INPUT_PORT)
            .context("Effector operation has no Shape input")?;
        assert_eq!(input.direction, PortDirection::Input);
        assert_eq!(input.data_type, PortDataType::Shape);
        assert_eq!(input.multiplicity, PortMultiplicity::Single);
        let output = operation
            .declared_ports
            .iter()
            .find(|port| port.key == SHAPE_OUTPUT_PORT)
            .context("Effector operation has no Shape output")?;
        assert_eq!(output.direction, PortDirection::Output);
        assert_eq!(output.side, PortSide::Right);
        assert_eq!(output.data_type, PortDataType::Shape);
    }

    let transform = plugins.create_effector_operation_node("transform")?;
    for key in ["tx", "ty", "scale_x", "scale_y", "rotation", "target"] {
        assert!(transform.properties().get(key).is_some(), "missing {key}");
    }
    let opacity = plugins.create_effector_operation_node("opacity")?;
    for key in ["opacity", "mode", "target"] {
        assert!(opacity.properties().get(key).is_some(), "missing {key}");
    }
    let tracking = plugins.create_effector_operation_node("tracking")?;
    for key in ["amount", "target"] {
        assert!(tracking.properties().get(key).is_some(), "missing {key}");
    }

    let manager = ProjectManager::new(Arc::new(RwLock::new(Project::new("factory"))), plugins);
    let text = manager
        .create_text_node("typed", "Arial", WIDTH, HEIGHT)
        .context("create Text source")?;
    let shape = manager
        .create_shape_node("M0 0 L10 0 L10 10 Z", WIDTH, HEIGHT, 10, 10)
        .context("create Shape source")?;
    let (mut project, composition_id, _) = setup_project();
    let text_id = text.id;
    let shape_id = shape.id;
    project.add_node(text);
    project.add_node(shape);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), text_id)
        .context("attach Text source to Composition")?;
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), shape_id)
        .context("attach Shape source to Composition")?;
    for source in [text_id, shape_id] {
        let output = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(source), SHAPE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .context("source has no Shape output port")?;
        assert_eq!(output.data_type, PortDataType::Shape);
        assert_eq!(output.multiplicity, PortMultiplicity::Single);
        assert_eq!(output.side, PortSide::Right);
    }
    Ok(())
}
