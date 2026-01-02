use crate::error::LibraryError;
use crate::model::project::Project;
use crate::model::property::{Keyframe, Property, PropertyValue};
use crate::model::{EffectConfig, Layer, LayerContent, Node, ReferenceContent};
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct ClipHandler;

impl ClipHandler {
    /// Add a clip to a track at a specific index (or index 0 if not specified)
    pub fn add_clip_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        layer: Layer,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        // Validation: Prevent circular references if adding a composition
        if let LayerContent::Reference(ReferenceContent { target_id, .. }) = &layer.content {
            if !Self::validate_recursion(project, *target_id, composition_id) {
                return Err(LibraryError::Project(
                    "Cannot add composition: Circular reference detected".to_string(),
                ));
            }
        }

        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // Ensure track exists
        if proj.get_track(track_id).is_none() {
            return Err(LibraryError::Project(format!(
                "Track with ID {} not found",
                track_id
            )));
        }

        let layer_id = layer.id;

        // Add clip to nodes registry
        proj.add_node(Node::Layer(layer));

        // Add clip ID to track's children at specified index (or 0 for top of layer list)
        if let Some(track) = proj.get_track_mut(track_id) {
            let idx = insert_index.unwrap_or(0);
            if idx <= track.children.len() {
                track.children.insert(idx, layer_id);
            } else {
                track.children.push(layer_id);
            }
        }

        Ok(layer_id)
    }

    /// Remove a clip from a track
    pub fn remove_clip_from_track(
        project: &Arc<RwLock<Project>>,
        track_id: Uuid,
        layer_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // Remove from parent track's child_ids
        if let Some(track) = proj.get_track_mut(track_id) {
            if track.children.contains(&layer_id) {
                track.children.retain(|&id| id != layer_id);
            } else {
                return Err(LibraryError::Project(format!(
                    "Layer {} not found in track {}",
                    layer_id, track_id
                )));
            }
        } else {
            return Err(LibraryError::Project(format!(
                "Track {} not found",
                track_id
            )));
        }

        // Remove from nodes registry
        proj.remove_node(layer_id);
        Ok(())
    }

    /// Unified method to update property or keyframe for any target
    pub fn update_target_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        target: crate::model::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        // Special handling for Clip struct fields sync
        if let crate::model::property::PropertyTarget::Clip = target {
            match property_key {
                "start_time" => {
                    if let PropertyValue::Number(n) = &value {
                        clip.start_time = OrderedFloat(n.into_inner());
                    }
                }
                "duration" => {
                    if let PropertyValue::Number(n) = &value {
                        clip.duration = OrderedFloat(n.into_inner());
                    }
                }
                "trim_in" => {
                    if let PropertyValue::Number(n) = &value {
                        clip.trim_in = OrderedFloat(n.into_inner());
                    }
                }
                "time_stretch" => {
                    if let PropertyValue::Number(n) = &value {
                        clip.time_stretch = OrderedFloat(n.into_inner());
                    }
                }
                _ => {}
            }
        }

        let prop_map = clip
            .get_property_map_mut(target.clone())
            .ok_or_else(|| LibraryError::Project(format!("Target {:?} not found", target)))?;

        prop_map.update_property_or_keyframe(property_key, time, value, easing);

        Ok(())
    }

    pub fn update_keyframe(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let property = clip
            .properties
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

        if let Some(PropertyValue::Array(promoted_array)) = property.properties.get_mut("keyframes")
        {
            let mut keyframes: Vec<Keyframe> = promoted_array
                .iter()
                .filter_map(|v| serde_json::from_value(serde_json::Value::from(v)).ok())
                .collect();

            if let Some(kf) = keyframes.get_mut(keyframe_index) {
                if let Some(t) = new_time {
                    kf.time = OrderedFloat(t);
                }
                if let Some(val) = new_value {
                    kf.value = val;
                }
                if let Some(easing) = new_easing {
                    kf.easing = easing;
                }
            } else {
                return Err(LibraryError::Project(
                    "Keyframe index out of bounds".to_string(),
                ));
            }

            keyframes.sort_by(|a, b| a.time.cmp(&b.time));

            let new_array: Vec<PropertyValue> = keyframes
                .into_iter()
                .filter_map(|kf| serde_json::to_value(kf).ok())
                .map(PropertyValue::from)
                .collect();

            promoted_array.clear();
            promoted_array.extend(new_array);
        }
        Ok(())
    }

    pub fn remove_keyframe(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        if let Some(prop) = clip.properties.get_mut(property_key) {
            if prop.evaluator == "keyframe" {
                let mut current_keyframes = prop.keyframes();
                if index < current_keyframes.len() {
                    current_keyframes.remove(index);
                    *prop = Property::keyframe(current_keyframes);
                }
            }
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Property {} not found",
                property_key
            )))
        }
    }

    fn validate_recursion(project: &Arc<RwLock<Project>>, child_id: Uuid, parent_id: Uuid) -> bool {
        if child_id == parent_id {
            return false;
        }
        let project_read = match project.read() {
            Ok(p) => p,
            Err(_) => return false,
        };

        let mut stack = vec![child_id];
        let mut visited = std::collections::HashSet::new();

        while let Some(current_id) = stack.pop() {
            if !visited.insert(current_id) {
                continue;
            }

            if let Some(comp) = project_read
                .compositions
                .iter()
                .find(|c| c.id == current_id)
            {
                // Traverse all nodes in the project (simplest for now, though expensive)
                // Ideally we only traverse nodes belonging to this Composite/Scope.
                // But since Registry is global, we need to know which nodes belong to root_track_id of Composite.
                // For now, simpler check: find any Reference in the nodes that points to parent_id.
                // Actually, wait. We need to find references INSIDE `current_id` (the composite we are inspecting).
                // `collect_clips` is gone. We have to walk the graph starting from `comp.root_track_id`.

                let mut node_stack = vec![comp.root_track_id];
                while let Some(node_id) = node_stack.pop() {
                    if let Some(node) = project_read.get_node(node_id) {
                        match node {
                            Node::Track(t) => {
                                node_stack.extend(t.children.iter());
                            }
                            Node::Layer(l) => {
                                if let LayerContent::Reference(ReferenceContent {
                                    target_id, ..
                                }) = &l.content
                                {
                                    if *target_id == parent_id {
                                        return false;
                                    }
                                    stack.push(*target_id);
                                }
                            }
                        }
                    }
                }
            }
        }
        true
    }

    pub fn move_clip_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
    ) -> Result<(), LibraryError> {
        Self::move_clip_to_track_at_index(
            project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_start_time,
            None,
        )
    }

    pub fn move_clip_to_track_at_index(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // 1. Remove from source track's child_ids
        if let Some(source_track) = proj.get_track_mut(source_track_id) {
            if source_track.children.contains(&clip_id) {
                source_track.children.retain(|&id| id != clip_id);
            } else {
                return Err(LibraryError::Project(format!(
                    "Clip {} not found in source track",
                    clip_id
                )));
            }
        } else {
            return Err(LibraryError::Project(format!(
                "Source track {} not found",
                source_track_id
            )));
        }

        // 2. Update clip timing
        if let Some(clip) = proj.get_layer_mut(clip_id) {
            clip.start_time = OrderedFloat(new_start_time);
        }

        // 3. Add to target track's child_ids
        if let Some(target_track) = proj.get_track_mut(target_track_id) {
            if let Some(idx) = target_index {
                if idx <= target_track.children.len() {
                    target_track.children.insert(idx, clip_id);
                } else {
                    target_track.children.push(clip_id);
                }
            } else {
                target_track.children.push(clip_id);
            }
        } else {
            return Err(LibraryError::Project(format!(
                "Target track {} not found",
                target_track_id
            )));
        }

        Ok(())
    }

    pub fn add_effect(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        effect: EffectConfig,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.effects.push(effect);
        Ok(())
    }

    pub fn update_effects(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        effects: Vec<EffectConfig>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.effects = effects;
        Ok(())
    }

    pub fn update_styles(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        styles: Vec<crate::model::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.styles = styles;
        Ok(())
    }

    pub fn set_style_property_attribute(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        if let Some(style) = clip.styles.get_mut(style_index) {
            if let Some(prop) = style.properties.get_mut(property_key) {
                prop.properties
                    .insert(attribute_key.to_string(), attribute_value);
                Ok(())
            } else {
                Err(LibraryError::Project(format!(
                    "Property {} not found",
                    property_key
                )))
            }
        } else {
            Err(LibraryError::Project(
                "Style index out of range".to_string(),
            ))
        }
    }

    pub fn set_clip_property_attribute(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        if let Some(prop) = clip.properties.get_mut(property_key) {
            prop.properties
                .insert(attribute_key.to_string(), attribute_value);
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Property {} not found",
                property_key
            )))
        }
    }

    pub fn set_effect_property_attribute(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_layer_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        if let Some(effect) = clip.effects.get_mut(effect_index) {
            if let Some(prop) = effect.properties.get_mut(property_key) {
                prop.properties
                    .insert(attribute_key.to_string(), attribute_value);
                Ok(())
            } else {
                Err(LibraryError::Project(format!(
                    "Property {} not found",
                    property_key
                )))
            }
        } else {
            Err(LibraryError::Project(
                "Effect index out of range".to_string(),
            ))
        }
    }
}
