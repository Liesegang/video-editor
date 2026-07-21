use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::error::LibraryError;
use crate::model::Node;
use crate::model::project::{
    NodeContainer, PortAddress, PortDataType, PortDirection, PortOwner, Project, SHAPE_INPUT_PORT,
    SHAPE_OUTPUT_PORT,
};
use crate::model::property::{Property, PropertyMap, PropertyValue};
use crate::plugin::property_port_key;

pub(super) fn connected_property_source(
    project: &Project,
    node_id: Uuid,
    key: &str,
) -> Option<PortAddress> {
    let target = PortAddress::new(PortOwner::Node(node_id), property_port_key(key));
    let mut incoming = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .collect::<Vec<_>>();
    incoming.sort_by_key(|connection| connection.id);
    incoming.first().map(|connection| connection.from.clone())
}

pub(super) fn scale_number_property(
    property: &Property,
    factor: f64,
) -> Result<Property, LibraryError> {
    match property.evaluator.as_str() {
        "constant" => property
            .value()
            .and_then(|value| value.get_as::<f64>())
            .map(|value| Property::constant(PropertyValue::Number(OrderedFloat(value * factor))))
            .ok_or_else(|| {
                LibraryError::Project("Legacy opacity constant is not numeric".to_string())
            }),
        "keyframe" => {
            let keyframes = property
                .keyframes()
                .into_iter()
                .map(|mut keyframe| {
                    let value = keyframe.value.get_as::<f64>().ok_or_else(|| {
                        LibraryError::Project(
                            "Legacy opacity keyframe contains a non-numeric value".to_string(),
                        )
                    })?;
                    keyframe.value = PropertyValue::Number(OrderedFloat(value * factor));
                    Ok(keyframe)
                })
                .collect::<Result<Vec<_>, LibraryError>>()?;
            let mut scaled = Property::keyframe(keyframes);
            for (key, value) in &property.properties {
                if key != "keyframes" && key != "value" {
                    scaled.properties.insert(key.clone(), value.clone());
                }
            }
            Ok(scaled)
        }
        "expression" => {
            let source = property.expression_text().ok_or_else(|| {
                LibraryError::Project("Legacy opacity Expression has no source".to_string())
            })?;
            let fallback = property
                .value()
                .and_then(|value| value.get_as::<f64>())
                .ok_or_else(|| {
                    LibraryError::Project(
                        "Legacy opacity Expression has no numeric fallback".to_string(),
                    )
                })?;
            let mut scaled = property.clone();
            scaled.properties.insert(
                "expression".to_string(),
                PropertyValue::String(format!("({source}) * {factor}")),
            );
            scaled.properties.insert(
                "value".to_string(),
                PropertyValue::Number(OrderedFloat(fallback * factor)),
            );
            Ok(scaled)
        }
        evaluator => Err(LibraryError::Project(format!(
            "Legacy opacity evaluator {evaluator:?} cannot be converted safely"
        ))),
    }
}

pub(super) fn is_neutral_legacy(key: &str, property: &Property) -> bool {
    let Some(value) = property.get_static_value() else {
        return false;
    };
    match key {
        "position" | "anchor" => value
            .get_as::<crate::model::property::Vec2>()
            .is_some_and(|value| value.x.into_inner() == 0.0 && value.y.into_inner() == 0.0),
        "scale" => value
            .get_as::<crate::model::property::Vec2>()
            .is_some_and(|value| value.x.into_inner() == 100.0 && value.y.into_inner() == 100.0),
        "rotation" => value.get_as::<f64>() == Some(0.0),
        "opacity" => value.get_as::<f64>() == Some(100.0),
        _ => false,
    }
}

pub(super) fn is_default_graph_property(key: &str, property: &Property) -> bool {
    default_graph_property(key).is_ok_and(|default| default == *property)
}

pub(super) fn default_graph_property(key: &str) -> Result<Property, LibraryError> {
    let definitions = if key == "opacity" {
        crate::plugin::styles::image_opacity_property_definitions()
    } else {
        crate::plugin::transforms::property_definitions()
    };
    definitions
        .into_iter()
        .find(|definition| definition.name() == key)
        .map(|definition| Property::constant(definition.default_value().clone()))
        .ok_or_else(|| LibraryError::Project(format!("No semantic definition for {key}")))
}

pub(super) fn conflicting_authority(node_id: Uuid, key: &str) -> LibraryError {
    LibraryError::Project(format!(
        "Legacy container {key} conflicts with authored semantic Node {node_id}; edit the exact Node or clear one authority"
    ))
}

pub(super) fn validate_candidate(
    project: &Project,
    owner: NodeContainer,
) -> Result<(), LibraryError> {
    let errors = project.validate_connections();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(LibraryError::Validation(format!(
            "Semantic graph transaction for {owner:?} is invalid: {}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )))
    }
}

pub(super) fn container_port_owner(owner: NodeContainer) -> PortOwner {
    match owner {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

pub(super) fn container_node_ids(
    project: &Project,
    owner: NodeContainer,
) -> Result<&[Uuid], LibraryError> {
    match owner {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.node_ids.as_slice()),
        NodeContainer::Track(id) => project.get_track(id).map(|track| track.node_ids.as_slice()),
        NodeContainer::Clip(id) => project.get_clip(id).map(|clip| clip.node_ids.as_slice()),
    }
    .ok_or_else(|| LibraryError::Project(format!("Semantic container {owner:?} not found")))
}

pub(super) fn container_properties(
    project: &Project,
    owner: NodeContainer,
) -> Result<&PropertyMap, LibraryError> {
    match owner {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| &composition.properties),
        NodeContainer::Track(id) => project.get_track(id).map(|track| &track.properties),
        NodeContainer::Clip(id) => project.get_clip(id).map(|clip| &clip.properties),
    }
    .ok_or_else(|| LibraryError::Project(format!("Semantic container {owner:?} not found")))
}

pub(super) fn container_properties_mut(
    project: &mut Project,
    owner: NodeContainer,
) -> Result<&mut PropertyMap, LibraryError> {
    match owner {
        NodeContainer::Composition(id) => project
            .get_composition_mut(id)
            .map(|composition| &mut composition.properties),
        NodeContainer::Track(id) => project.get_track_mut(id).map(|track| &mut track.properties),
        NodeContainer::Clip(id) => project.get_clip_mut(id).map(|clip| &mut clip.properties),
    }
    .ok_or_else(|| LibraryError::Project(format!("Semantic container {owner:?} not found")))
}

pub(super) fn container_output_node_id(
    project: &Project,
    owner: NodeContainer,
) -> Result<Option<Uuid>, LibraryError> {
    match owner {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.output_node_id),
        NodeContainer::Track(id) => project.get_track(id).map(|track| track.output_node_id),
        NodeContainer::Clip(id) => project.get_clip(id).map(|clip| clip.output_node_id),
    }
    .ok_or_else(|| LibraryError::Project(format!("Semantic container {owner:?} not found")))
}

pub(super) fn position_after_source(
    project: &Project,
    node: &mut Node,
    source: &PortAddress,
    offset: f32,
) {
    if let PortOwner::Node(source_id) = source.owner
        && let Some(source) = project.get_node(source_id)
    {
        node.ui_position = [source.ui_position[0] + offset, source.ui_position[1]];
    }
}

/// Finds the one terminal primary Shape flow when a container has not yet
/// been rasterized by a Style. Secondary Shape inputs (for example Backplate
/// geometry) do not redefine the terminal semantic source.
pub(super) fn terminal_shape_source(
    project: &Project,
    owner: NodeContainer,
) -> Result<PortAddress, LibraryError> {
    let contained = container_node_ids(project, owner)?
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut candidates = contained
        .iter()
        .copied()
        .filter(|node_id| {
            project
                .port_definition(
                    &PortAddress::new(PortOwner::Node(*node_id), SHAPE_OUTPUT_PORT),
                    PortDirection::Output,
                )
                .is_some_and(|port| port.data_type == PortDataType::Shape)
        })
        .filter(|node_id| {
            let output = PortAddress::new(PortOwner::Node(*node_id), SHAPE_OUTPUT_PORT);
            !project.connections.iter().any(|connection| {
                connection.from == output
                    && connection.to.port == SHAPE_INPUT_PORT
                    && matches!(connection.to.owner, PortOwner::Node(target) if contained.contains(&target))
                    && project
                        .port_definition(
                            &PortAddress::new(connection.to.owner, SHAPE_OUTPUT_PORT),
                            PortDirection::Output,
                        )
                        .is_some_and(|port| port.data_type == PortDataType::Shape)
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    let [node_id] = candidates.as_slice() else {
        return Err(LibraryError::Project(format!(
            "Cannot find one terminal Shape source for {owner:?}; candidates={}",
            candidates
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )));
    };
    Ok(PortAddress::new(
        PortOwner::Node(*node_id),
        SHAPE_OUTPUT_PORT,
    ))
}
