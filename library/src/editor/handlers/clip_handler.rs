use crate::error::LibraryError;
use crate::model::project::project::Project;
use crate::model::project::property::{Keyframe, Property, PropertyValue};
use crate::model::project::{Node, TrackClip, TrackClipKind};
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
        clip: TrackClip,
        in_frame: u64,
        out_frame: u64,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        // Validation: Prevent circular references if adding a composition
        if clip.kind == TrackClipKind::Composition {
            if let Some(ref_id) = clip.reference_id {
                if !Self::validate_recursion(project, ref_id, composition_id) {
                    return Err(LibraryError::Project(
                        "Cannot add composition: Circular reference detected".to_string(),
                    ));
                }
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

        let clip_id = clip.id;
        let mut final_clip = clip;
        final_clip.in_frame = in_frame;
        final_clip.out_frame = out_frame;

        // Add clip to nodes registry
        proj.add_node(Node::Clip(final_clip));

        // Add clip ID to track's children at specified index (or 0 for top of layer list)
        if let Some(track) = proj.get_track_mut(track_id) {
            let idx = insert_index.unwrap_or(0);
            track.insert_child(idx, clip_id);
        }

        Ok(clip_id)
    }

    /// Remove a clip from a track
    pub fn remove_clip_from_track(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // Remove from parent track's child_ids
        if let Some(track) = proj.get_track_mut(track_id) {
            if !track.remove_child(clip_id) {
                return Err(LibraryError::Project(format!(
                    "Clip {} not found in track {}",
                    clip_id, track_id
                )));
            }
        } else {
            return Err(LibraryError::Project(format!(
                "Track {} not found",
                track_id
            )));
        }

        // Remove from nodes registry
        proj.remove_node(clip_id);
        Ok(())
    }

    pub fn update_clip_property(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        // Sync struct fields with property updates
        match key {
            "in_frame" => {
                if let PropertyValue::Number(n) = &value {
                    clip.in_frame = n.into_inner().round() as u64;
                }
            }
            "out_frame" => {
                if let PropertyValue::Number(n) = &value {
                    clip.out_frame = n.into_inner().round() as u64;
                }
            }
            "source_begin_frame" => {
                if let PropertyValue::Number(n) = &value {
                    clip.source_begin_frame = n.into_inner().round() as i64;
                }
            }
            _ => {}
        }

        clip.properties
            .set(key.to_string(), Property::constant(value));
        Ok(())
    }

    pub fn update_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        // Get or create property
        if let Some(prop) = clip.properties.get_mut(property_key) {
            if prop.evaluator == "keyframe" {
                prop.upsert_keyframe(time, value, easing);
            } else {
                clip.properties
                    .set(property_key.to_string(), Property::constant(value));
            }
        } else {
            clip.properties
                .set(property_key.to_string(), Property::constant(value));
        }
        Ok(())
    }

    pub fn update_keyframe(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
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
            .get_clip_mut(clip_id)
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
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
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
                // Collect all clips from the root track
                for clip in project_read.collect_clips(comp.root_track_id) {
                    if clip.kind == TrackClipKind::Composition {
                        if let Some(ref_id) = clip.reference_id {
                            if ref_id == parent_id {
                                return false;
                            }
                            stack.push(ref_id);
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
        new_in_frame: u64,
    ) -> Result<(), LibraryError> {
        Self::move_clip_to_track_at_index(
            project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_in_frame,
            None,
        )
    }

    pub fn move_clip_to_track_at_index(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // 1. Remove from source track's child_ids
        if let Some(source_track) = proj.get_track_mut(source_track_id) {
            if !source_track.remove_child(clip_id) {
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
        if let Some(clip) = proj.get_clip_mut(clip_id) {
            let duration = clip.out_frame - clip.in_frame;
            clip.in_frame = new_in_frame;
            clip.out_frame = new_in_frame + duration;
        }

        // 3. Add to target track's child_ids
        if let Some(target_track) = proj.get_track_mut(target_track_id) {
            if let Some(idx) = target_index {
                target_track.insert_child(idx, clip_id);
            } else {
                target_track.add_child(clip_id);
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
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        effect: crate::model::project::EffectConfig,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.effects.push(effect);
        Ok(())
    }

    pub fn update_effects(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        effects: Vec<crate::model::project::EffectConfig>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.effects = effects;
        Ok(())
    }

    pub fn update_styles(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        styles: Vec<crate::model::project::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.styles = styles;
        Ok(())
    }

    pub fn update_style_property(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        if let Some(style) = clip.styles.get_mut(style_index) {
            style
                .properties
                .set(property_key.to_string(), Property::constant(value));
            Ok(())
        } else {
            Err(LibraryError::Project(
                "Style index out of range".to_string(),
            ))
        }
    }

    pub fn update_effect_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        clip.update_effect_property(effect_index, property_key, time, value, easing)
            .map_err(|e| LibraryError::Project(e.to_string()))
    }

    pub fn update_style_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        clip.update_style_property(style_index, property_key, time, value, easing)
            .map_err(|e| LibraryError::Project(e.to_string()))
    }

    pub fn set_style_property_attribute(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
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
            .get_clip_mut(clip_id)
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
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
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
        _composition_id: Uuid,
        _track_id: Uuid,
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
            .get_clip_mut(clip_id)
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
