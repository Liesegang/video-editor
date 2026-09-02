use std::collections::{HashMap, HashSet};

use ordered_float::OrderedFloat;

use crate::model::authoring::{
    GeneratedItem, GeneratedItemId, GeneratedItemSpec, Override, OverrideOperator, OverridePath,
    OverrideStatus, SourceRef, TimelineItem, TimelineItemId, TimelineTrackId,
};
use crate::model::project::property::PropertyValue;
use crate::model::project::property::{Property, PropertyMap};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReconciliationSummary {
    pub active: usize,
    pub orphaned: usize,
    pub conflicts: usize,
}

pub fn reconcile_generation(
    generated_items: &mut HashMap<GeneratedItemId, GeneratedItem>,
    overrides: &mut HashMap<crate::model::authoring::OverrideId, Override>,
    regenerated: Vec<GeneratedItem>,
) -> Result<ReconciliationSummary, String> {
    let mut next = HashMap::new();
    for item in regenerated {
        let expected = GeneratedItem::stable_id(item.generator_id, &item.source_key);
        if item.stable_id != expected {
            return Err(format!(
                "Generated item {} has an unstable provenance ID",
                item.stable_id
            ));
        }
        if next.insert(item.stable_id, item).is_some() {
            return Err("Generator produced duplicate stable source keys".to_string());
        }
    }

    let mut summary = ReconciliationSummary::default();
    for authored_override in overrides.values_mut() {
        let Some(item) = next.get(&authored_override.generated_item_id) else {
            authored_override.status = OverrideStatus::Orphaned;
            summary.orphaned += 1;
            continue;
        };
        match validate_override(item, authored_override) {
            Ok(()) => {
                authored_override.status = OverrideStatus::Active;
                summary.active += 1;
            }
            Err(reason) => {
                authored_override.status = OverrideStatus::Conflict { reason };
                summary.conflicts += 1;
            }
        }
    }
    *generated_items = next;
    Ok(summary)
}

pub fn reconcile_and_materialize(
    project: &mut crate::model::authoring::AuthoringProject,
    track_id: TimelineTrackId,
    generator_id: crate::model::authoring::ModuleInstanceId,
    regenerated: Vec<GeneratedItem>,
) -> Result<ReconciliationSummary, String> {
    if !project.tracks.contains_key(&track_id) {
        return Err(format!("Generator target Track {track_id} is missing"));
    }
    if regenerated
        .iter()
        .any(|item| item.generator_id != generator_id)
    {
        return Err("Generator result contains another Generator instance".to_string());
    }
    let previous_ids: HashSet<_> = project
        .generated_items
        .values()
        .filter(|item| item.generator_id == generator_id)
        .map(|item| item.stable_id)
        .collect();
    let regenerated_ids: HashSet<_> = regenerated.iter().map(|item| item.stable_id).collect();
    let scoped_ids: HashSet<_> = previous_ids.union(&regenerated_ids).copied().collect();
    let mut scoped_items: HashMap<_, _> = previous_ids
        .iter()
        .filter_map(|id| project.generated_items.remove(id).map(|item| (*id, item)))
        .collect();
    let mut scoped_overrides: HashMap<_, _> = project
        .overrides
        .iter()
        .filter(|(_, authored_override)| scoped_ids.contains(&authored_override.generated_item_id))
        .map(|(id, authored_override)| (*id, authored_override.clone()))
        .collect();
    let summary = reconcile_generation(&mut scoped_items, &mut scoped_overrides, regenerated)?;
    project.generated_items.extend(scoped_items);
    project.overrides.extend(scoped_overrides);
    for generated_id in previous_ids {
        if !project.generated_items.contains_key(&generated_id) {
            project
                .items
                .remove(&TimelineItemId::from_uuid(generated_id.as_uuid()));
        }
    }
    let generated: Vec<_> = project
        .generated_items
        .values()
        .filter(|item| item.generator_id == generator_id)
        .cloned()
        .collect();
    for generated_item in generated {
        let item_id = TimelineItemId::from_uuid(generated_item.stable_id.as_uuid());
        if project
            .items
            .get(&item_id)
            .is_some_and(|item| item.generated_item_id != Some(generated_item.stable_id))
        {
            return Err(format!(
                "Generated item {} collides with an authored Timeline item",
                generated_item.stable_id
            ));
        }
        let spec = effective_generated_spec(
            &generated_item,
            project.overrides.values().filter(|authored_override| {
                authored_override.generated_item_id == generated_item.stable_id
            }),
        )?;
        let mut properties = PropertyMap::new();
        for (key, value) in spec.authored_values {
            properties.set(key, Property::constant(value));
        }
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: spec.name,
                source: spec.source,
                interval: spec.interval,
                layer: spec.layer,
                parent: None,
                mask_ids: Vec::new(),
                matte: None,
                constraints: Vec::new(),
                transition_in: None,
                transition_out: None,
                generated_item_id: Some(generated_item.stable_id),
                authored_properties: properties,
            },
        );
    }
    project.validate()?;
    Ok(summary)
}

pub fn effective_generated_spec<'a>(
    item: &GeneratedItem,
    overrides: impl Iterator<Item = &'a Override>,
) -> Result<GeneratedItemSpec, String> {
    let mut spec = item.generated_spec.clone();
    for authored_override in overrides {
        if authored_override.generated_item_id != item.stable_id
            || authored_override.status != OverrideStatus::Active
        {
            continue;
        }
        for patch in &authored_override.patch {
            apply_patch(&mut spec, patch)?;
        }
    }
    Ok(spec)
}

fn validate_override(item: &GeneratedItem, authored_override: &Override) -> Result<(), String> {
    let mut spec = item.generated_spec.clone();
    for patch in &authored_override.patch {
        apply_patch(&mut spec, patch)?;
    }
    Ok(())
}

fn apply_patch(
    spec: &mut GeneratedItemSpec,
    patch: &crate::model::authoring::OverridePatch,
) -> Result<(), String> {
    match &patch.path {
        OverridePath::SourceText => {
            let SourceRef::Text { text } = &mut spec.source else {
                return Err("Generated source is no longer Text".to_string());
            };
            if patch.operator != OverrideOperator::Replace {
                return Err("Text overrides only support Replace".to_string());
            }
            let PropertyValue::String(value) = &patch.value else {
                return Err("Text override value is not a String".to_string());
            };
            *text = value.clone();
            Ok(())
        }
        OverridePath::AuthoredProperty { key } => {
            let value = spec
                .authored_values
                .get_mut(key)
                .ok_or_else(|| format!("Generated property '{key}' no longer exists"))?;
            apply_value(value, patch.operator, &patch.value)
        }
        OverridePath::ModuleParameter { key } => {
            let value = spec
                .module_parameters
                .get_mut(key)
                .ok_or_else(|| format!("Generated Module parameter '{key}' no longer exists"))?;
            apply_value(value, patch.operator, &patch.value)
        }
    }
}

fn apply_value(
    target: &mut PropertyValue,
    operator: OverrideOperator,
    operand: &PropertyValue,
) -> Result<(), String> {
    if operator == OverrideOperator::Replace {
        if std::mem::discriminant(target) != std::mem::discriminant(operand) {
            return Err("Generated value type changed".to_string());
        }
        *target = operand.clone();
        return Ok(());
    }
    let current =
        numeric_value(target).ok_or_else(|| "Generated value is not numeric".to_string())?;
    let operand =
        numeric_value(operand).ok_or_else(|| "Override operand is not numeric".to_string())?;
    let result = match operator {
        OverrideOperator::Replace => operand,
        OverrideOperator::Add => current + operand,
        OverrideOperator::Multiply => current * operand,
    };
    *target = PropertyValue::Number(OrderedFloat(result));
    Ok(())
}

fn numeric_value(value: &PropertyValue) -> Option<f64> {
    match value {
        PropertyValue::Integer(value) => Some(*value as f64),
        PropertyValue::Number(value) => Some(value.into_inner()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::{
        AuthoringProject, AuthoringSession, GeneratedProvenance, ModuleDefinition,
        ModuleDefinitionId, ModuleGraph, ModuleInstance, ModuleInstanceId, ModuleRole, OverrideId,
        OverridePatch, TimelineInterval,
    };

    fn generated(generator: ModuleInstanceId, source_key: &str, x: f64) -> GeneratedItem {
        GeneratedItem {
            stable_id: GeneratedItem::stable_id(generator, source_key),
            generator_id: generator,
            generator_version: 2,
            source_key: source_key.to_string(),
            generated_spec: GeneratedItemSpec {
                name: source_key.to_string(),
                source: SourceRef::Text {
                    text: "Label".to_string(),
                },
                interval: TimelineInterval::new(0.0, 1.0).expect("valid interval"),
                layer: 0,
                authored_values: HashMap::from([(
                    "x".to_string(),
                    PropertyValue::Number(OrderedFloat(x)),
                )]),
                module_parameters: HashMap::new(),
            },
            provenance: GeneratedProvenance {
                data_source_id: None,
                source_fingerprint: "fixture".to_string(),
                generated_at_revision: 2,
            },
        }
    }

    #[test]
    fn stable_item_keeps_manual_offset_after_regeneration() {
        let generator = ModuleInstanceId::new();
        let first = generated(generator, "row-1", 10.0);
        let override_id = OverrideId::new();
        let mut generated_items = HashMap::from([(first.stable_id, first.clone())]);
        let mut overrides = HashMap::from([(
            override_id,
            Override {
                id: override_id,
                generated_item_id: first.stable_id,
                patch: vec![OverridePatch {
                    path: OverridePath::AuthoredProperty {
                        key: "x".to_string(),
                    },
                    operator: OverrideOperator::Add,
                    value: PropertyValue::Number(OrderedFloat(15.0)),
                }],
                status: OverrideStatus::Active,
            },
        )]);
        let refreshed = generated(generator, "row-1", 20.0);
        let summary = reconcile_generation(
            &mut generated_items,
            &mut overrides,
            vec![refreshed.clone()],
        )
        .expect("reconciliation");
        assert_eq!(summary.active, 1);
        let effective =
            effective_generated_spec(&refreshed, overrides.values()).expect("override must apply");
        assert_eq!(
            effective.authored_values["x"],
            PropertyValue::Number(OrderedFloat(35.0))
        );
    }

    #[test]
    fn removed_stable_item_preserves_orphaned_override() {
        let generator = ModuleInstanceId::new();
        let item = generated(generator, "deleted-row", 0.0);
        let override_id = OverrideId::new();
        let mut generated_items = HashMap::from([(item.stable_id, item.clone())]);
        let mut overrides = HashMap::from([(
            override_id,
            Override {
                id: override_id,
                generated_item_id: item.stable_id,
                patch: Vec::new(),
                status: OverrideStatus::Active,
            },
        )]);
        let summary = reconcile_generation(&mut generated_items, &mut overrides, Vec::new())
            .expect("reconciliation");
        assert_eq!(summary.orphaned, 1);
        assert_eq!(overrides[&override_id].status, OverrideStatus::Orphaned);
    }

    #[test]
    fn materialized_item_keeps_direct_edit_across_removal_and_return() {
        let mut project =
            AuthoringProject::new("Data", 1920, 1080, 30.0, 10.0).expect("valid project");
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let definition_id = ModuleDefinitionId::new();
        let generator_id = ModuleInstanceId::new();
        project.module_definitions.insert(
            definition_id,
            ModuleDefinition {
                id: definition_id,
                name: "Table rows".to_string(),
                role: ModuleRole::Generator,
                graph: ModuleGraph::default(),
                published_parameters: Vec::new(),
                published_signals: Vec::new(),
                published_actions: Vec::new(),
                version: 1,
            },
        );
        project.module_instances.insert(
            generator_id,
            ModuleInstance {
                id: generator_id,
                definition_id,
                parameter_overrides: HashMap::new(),
            },
        );

        let first = generated(generator_id, "row-1", 10.0);
        let stable_id = first.stable_id;
        reconcile_and_materialize(&mut project, track_id, generator_id, vec![first])
            .expect("initial generation");
        let item_id = TimelineItemId::from_uuid(stable_id.as_uuid());
        let mut session = AuthoringSession::new(project).expect("valid materialized project");
        session
            .update_item_property_value(
                item_id,
                "x".to_string(),
                0.0,
                PropertyValue::Number(OrderedFloat(25.0)),
            )
            .expect("direct edit");
        let mut project = session.into_project();

        reconcile_and_materialize(
            &mut project,
            track_id,
            generator_id,
            vec![generated(generator_id, "row-1", 20.0)],
        )
        .expect("refresh");
        assert_eq!(
            project.items[&item_id]
                .authored_properties
                .get("x")
                .and_then(|property| property.get_static_value()),
            Some(&PropertyValue::Number(OrderedFloat(25.0)))
        );

        let removed = reconcile_and_materialize(&mut project, track_id, generator_id, Vec::new())
            .expect("row removal");
        assert_eq!(removed.orphaned, 1);
        assert!(!project.items.contains_key(&item_id));
        assert!(
            project
                .overrides
                .values()
                .all(|authored_override| authored_override.status == OverrideStatus::Orphaned)
        );

        let restored = reconcile_and_materialize(
            &mut project,
            track_id,
            generator_id,
            vec![generated(generator_id, "row-1", 30.0)],
        )
        .expect("row restoration");
        assert_eq!(restored.active, 1);
        assert_eq!(
            project.items[&item_id]
                .authored_properties
                .get("x")
                .and_then(|property| property.get_static_value()),
            Some(&PropertyValue::Number(OrderedFloat(25.0)))
        );
    }
}
