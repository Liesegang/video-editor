use std::collections::HashMap;

use ordered_float::OrderedFloat;

use crate::model::authoring::{
    GeneratedItem, GeneratedItemId, GeneratedItemSpec, Override, OverrideOperator, OverridePath,
    OverrideStatus, SourceRef,
};
use crate::model::project::property::PropertyValue;

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
        GeneratedProvenance, ModuleInstanceId, OverrideId, OverridePatch, TimelineInterval,
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
}
