//! Derived Point attribute schema and semantic Store-name validation.

use crate::model::authoring::ModuleDefinition;
use crate::model::node::PointNodeRole;
use crate::model::point::{PointAttributeDefinition, PointAttributeId, PointAttributeSchema};

use super::{particle_sprite, point_role, trace_point_stream};

/// Validates the semantic display name of one Store Attribute Node against
/// every current Point stream that contains it. Independent Point domains may
/// intentionally reuse a display name; an unconnected Store has no stream yet
/// but still follows the authoritative structural name rules.
pub fn validate_module_node_name(
    definition: &ModuleDefinition,
    node_id: uuid::Uuid,
    proposed_name: &str,
) -> Result<(), String> {
    let node = definition
        .graph
        .nodes
        .get(&node_id)
        .ok_or_else(|| format!("Missing Module Node {node_id}"))?;
    let Some(element_type) = point_role(node).and_then(PointNodeRole::attribute_type) else {
        return Ok(());
    };
    PointAttributeDefinition::new(
        PointAttributeId::from_uuid(node.id),
        proposed_name.to_string(),
        element_type,
        element_type.default_value(),
    )?;

    for renderer in definition.graph.nodes.values() {
        if !particle_sprite(renderer) {
            continue;
        }
        let Some(trace) = trace_point_stream(definition, renderer.id)? else {
            continue;
        };
        let stores = trace.stores().collect::<Vec<_>>();
        if stores.contains(&node_id) {
            point_attribute_schema_with_name(definition, &stores, Some((node_id, proposed_name)))?;
        }
    }
    Ok(())
}

pub(super) fn point_attribute_schema(
    definition: &ModuleDefinition,
    stores: &[uuid::Uuid],
) -> Result<PointAttributeSchema, String> {
    point_attribute_schema_with_name(definition, stores, None)
}

fn point_attribute_schema_with_name(
    definition: &ModuleDefinition,
    stores: &[uuid::Uuid],
    proposed_name: Option<(uuid::Uuid, &str)>,
) -> Result<PointAttributeSchema, String> {
    let mut definitions = Vec::with_capacity(stores.len());
    for store_id in stores {
        let store =
            definition.graph.nodes.get(store_id).ok_or_else(|| {
                format!("Point Store Node {store_id} disappeared during compilation")
            })?;
        let element_type = point_role(store)
            .and_then(PointNodeRole::attribute_type)
            .ok_or_else(|| format!("Point Store Node {store_id} has no attribute type"))?;
        definitions.push(PointAttributeDefinition::new(
            PointAttributeId::from_uuid(store.id),
            proposed_name
                .filter(|(node_id, _)| *node_id == store.id)
                .map_or_else(|| store.name.clone(), |(_, name)| name.to_string()),
            element_type,
            element_type.default_value(),
        )?);
    }
    PointAttributeSchema::new(definitions)
}
