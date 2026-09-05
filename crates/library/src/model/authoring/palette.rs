use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::property::{ColorValue, GradientValue, PatternValue};

use super::{PaintDefinitionId, PaletteGroupId};

/// A reusable authored paint. Every variant retains managed colors and exact
/// typed geometry; Palette selection never flattens a Gradient or Pattern to
/// a representative Solid swatch.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Paint {
    Solid(ColorValue),
    Gradient(GradientValue),
    Pattern(PatternValue),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PaintDefinition {
    pub id: PaintDefinitionId,
    pub name: String,
    pub paint: Paint,
    pub tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PaletteGroup {
    pub id: PaletteGroupId,
    pub name: String,
    pub definition_order: Vec<PaintDefinitionId>,
}

/// Project-owned reusable paints with explicit presentation order.
///
/// Definitions and ordering are intentionally separate: dragging a swatch
/// never changes its stable identity or external references.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct ProjectPalette {
    pub definitions: HashMap<PaintDefinitionId, PaintDefinition>,
    pub groups: Vec<PaletteGroup>,
    pub ungrouped_order: Vec<PaintDefinitionId>,
}

impl ProjectPalette {
    pub fn validate(&self) -> Result<(), String> {
        let mut ordered_definition_ids = HashSet::new();
        for definition_id in &self.ungrouped_order {
            validate_ordered_definition(
                &self.definitions,
                &mut ordered_definition_ids,
                *definition_id,
            )?;
        }

        let mut group_ids = HashSet::new();
        for group in &self.groups {
            if group.id.as_uuid().is_nil() || !group_ids.insert(group.id) {
                return Err("Project Palette contains a duplicate or nil group ID".to_string());
            }
            if group.name.trim().is_empty() || group.name.trim() != group.name {
                return Err(format!("Palette group {} has an invalid name", group.id));
            }
            for definition_id in &group.definition_order {
                validate_ordered_definition(
                    &self.definitions,
                    &mut ordered_definition_ids,
                    *definition_id,
                )?;
            }
        }

        for (definition_id, definition) in &self.definitions {
            if *definition_id != definition.id || definition_id.as_uuid().is_nil() {
                return Err("Project Palette definition map key does not match its ID".to_string());
            }
            validate_definition(definition)?;
            if !ordered_definition_ids.contains(definition_id) {
                return Err(format!(
                    "Paint Definition {definition_id} is missing from Palette order"
                ));
            }
        }
        Ok(())
    }

    /// Definitions in the exact order shown by the ungrouped Palette UI.
    /// A validated Project guarantees that every ID resolves exactly once.
    pub fn ungrouped_definitions(&self) -> impl Iterator<Item = &PaintDefinition> {
        self.ungrouped_order
            .iter()
            .filter_map(|definition_id| self.definitions.get(definition_id))
    }

    /// Returns a lossless copy of one managed Solid color for picker use.
    pub fn solid_color(&self, definition_id: PaintDefinitionId) -> Option<ColorValue> {
        self.definitions
            .get(&definition_id)
            .and_then(|definition| match &definition.paint {
                Paint::Solid(color) => Some(color.clone()),
                Paint::Gradient(_) | Paint::Pattern(_) => None,
            })
    }
}

fn validate_ordered_definition(
    definitions: &HashMap<PaintDefinitionId, PaintDefinition>,
    ordered_definition_ids: &mut HashSet<PaintDefinitionId>,
    definition_id: PaintDefinitionId,
) -> Result<(), String> {
    if !definitions.contains_key(&definition_id) {
        return Err(format!(
            "Project Palette order references missing Paint Definition {definition_id}"
        ));
    }
    if !ordered_definition_ids.insert(definition_id) {
        return Err(format!(
            "Paint Definition {definition_id} appears more than once in Palette order"
        ));
    }
    Ok(())
}

fn validate_definition(definition: &PaintDefinition) -> Result<(), String> {
    if definition.name.trim().is_empty() || definition.name.trim() != definition.name {
        return Err(format!(
            "Paint Definition {} has an invalid name",
            definition.id
        ));
    }
    let mut normalized_tags = HashSet::new();
    for tag in &definition.tags {
        if tag.trim().is_empty() || tag.trim() != tag || !normalized_tags.insert(tag.to_lowercase())
        {
            return Err(format!(
                "Paint Definition {} has invalid or duplicate tags",
                definition.id
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::property::{ColorSpaceRef, ColorValue};

    fn definition() -> PaintDefinition {
        let id = PaintDefinitionId::new();
        PaintDefinition {
            id,
            name: "Accent".to_string(),
            paint: Paint::Solid(
                ColorValue::new(ColorSpaceRef::linear_srgb(), [2.0, -0.25, 0.5, 0.75]).unwrap(),
            ),
            tags: vec!["brand".to_string()],
        }
    }

    #[test]
    fn palette_requires_each_definition_in_exactly_one_order_location() {
        let definition = definition();
        let mut palette = ProjectPalette {
            definitions: HashMap::from([(definition.id, definition.clone())]),
            groups: Vec::new(),
            ungrouped_order: Vec::new(),
        };
        assert!(palette.validate().is_err());

        palette.ungrouped_order.push(definition.id);
        palette.groups.push(PaletteGroup {
            id: PaletteGroupId::new(),
            name: "Brand".to_string(),
            definition_order: vec![definition.id],
        });
        assert!(palette.validate().is_err());
    }

    #[test]
    fn palette_rejects_mismatched_identity_and_noncanonical_metadata() {
        let mut definition = definition();
        let wrong_map_id = PaintDefinitionId::new();
        let mut palette = ProjectPalette {
            definitions: HashMap::from([(wrong_map_id, definition.clone())]),
            groups: Vec::new(),
            ungrouped_order: vec![wrong_map_id],
        };
        assert!(palette.validate().is_err());

        definition.name = " Accent ".to_string();
        palette.definitions = HashMap::from([(definition.id, definition.clone())]);
        palette.ungrouped_order = vec![definition.id];
        assert!(palette.validate().is_err());

        definition.name = "Accent".to_string();
        definition.tags = vec!["Brand".to_string(), "brand".to_string()];
        palette.definitions = HashMap::from([(definition.id, definition)]);
        assert!(palette.validate().is_err());
    }

    #[test]
    fn solid_color_accessor_preserves_managed_components() {
        let definition = definition();
        let expected = definition.paint.clone();
        let palette = ProjectPalette {
            definitions: HashMap::from([(definition.id, definition.clone())]),
            groups: Vec::new(),
            ungrouped_order: vec![definition.id],
        };
        assert_eq!(
            palette.solid_color(definition.id).map(Paint::Solid),
            Some(expected)
        );
    }
}
