use crate::error::LibraryError;
use crate::model::project::project::Project;
use crate::model::project::property::{Keyframe, Property, PropertyValue};
use crate::model::project::{TrackClip, TrackClipKind};
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct ClipHandler;

impl ClipHandler {
    pub fn add_clip_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip: TrackClip,
        in_frame: u64,
        out_frame: u64,
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

        let track = proj
            .get_track_mut(composition_id, track_id)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Track with ID {} not found in Composition {}",
                    track_id, composition_id
                ))
            })?;

        let id = clip.id;
        // Ensure the clip's timing matches the requested timing
        let mut final_clip = clip;
        final_clip.in_frame = in_frame;
        final_clip.out_frame = out_frame;

        track.add_clip(final_clip);
        Ok(id)
    }

    pub fn remove_clip_from_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;
        let track = composition.get_track_mut(track_id).ok_or_else(|| {
            LibraryError::Project(format!(
                "Track with ID {} not found in Composition {}",
                track_id, composition_id
            ))
        })?;

        if let Some(index) = track.children.iter().position(|item| item.id() == clip_id) {
            track.children.remove(index);
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Clip with ID {} not found in track {}",
                clip_id, track_id
            )))
        }
    }

    pub fn update_clip_property(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let track = proj
            .get_track_mut(composition_id, track_id)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Track with ID {} not found in Composition {}",
                    track_id, composition_id
                ))
            })?;

        if let Some(clip) = track.clips_mut().find(|e| e.id == clip_id) {
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
        } else {
            Err(LibraryError::Project(format!(
                "Clip with ID {} not found",
                clip_id
            )))
        }
    }

    pub fn update_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
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
            .get_clip_mut(composition_id, track_id, clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        // Get or create property
        if let Some(prop) = clip.properties.get_mut(property_key) {
            if prop.evaluator == "keyframe" {
                // Use helper to upsert keyframe
                prop.upsert_keyframe(time, value, easing);
            } else {
                // Update as Constant
                clip.properties
                    .set(property_key.to_string(), Property::constant(value));
            }
        } else {
            // Property doesn't exist, create as constant
            clip.properties
                .set(property_key.to_string(), Property::constant(value));
        }
        Ok(())
    }

    pub fn update_keyframe(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
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
            .get_clip_mut(composition_id, track_id, clip_id)
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

            // Resort
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
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(composition_id, track_id, clip_id)
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
        // Copied Logic
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
                for track in &comp.tracks {
                    for clip in track.clips() {
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
        // Delegate to new method with push behavior (None index)
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
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;

        // 1. Extract Clip
        let source_track = composition.get_track_mut(source_track_id).ok_or_else(|| {
            LibraryError::Project(format!("Source Track {} not found", source_track_id))
        })?;

        let source_index = source_track
            .children
            .iter()
            .position(|item| item.id() == clip_id)
            .ok_or_else(|| {
                LibraryError::Project(format!("Clip {} not found in source track", clip_id))
            })?;

        let removed_item = source_track.children.remove(source_index);
        let mut clip = match removed_item {
            crate::model::project::TrackItem::Clip(c) => c,
            _ => {
                return Err(LibraryError::Project(
                    "Expected clip, found track".to_string(),
                ));
            }
        };

        // adjust timing
        let duration = clip.out_frame - clip.in_frame;
        clip.in_frame = new_in_frame;
        clip.out_frame = new_in_frame + duration;

        // 2. Insert into Target
        let mut final_target_index = target_index;

        // Adjust index if moving within same track
        if source_track_id == target_track_id {
            if let Some(idx) = final_target_index {
                // No adjustment needed - target_index is the final index in the simplified list
            }
            // Already have valid mutable reference to source (which is target)
            if let Some(idx) = final_target_index {
                source_track.insert_clip(idx, clip);
            } else {
                source_track.add_clip(clip);
            }
            Ok(())
        } else {
            let target_track = composition.get_track_mut(target_track_id).ok_or_else(|| {
                LibraryError::Project(format!("Target Track {} not found", target_track_id))
            })?;

            if let Some(idx) = final_target_index {
                target_track.insert_clip(idx, clip);
            } else {
                target_track.add_clip(clip);
            }
            Ok(())
        }
    }

    pub fn add_effect(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect: crate::model::project::EffectConfig,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(composition_id, track_id, clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.effects.push(effect);
        Ok(())
    }

    pub fn update_effects(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effects: Vec<crate::model::project::EffectConfig>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(composition_id, track_id, clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.effects = effects;
        Ok(())
    }

    pub fn update_styles(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        styles: Vec<crate::model::project::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(composition_id, track_id, clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        clip.styles = styles;
        Ok(())
    }

    pub fn update_style_property(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(composition_id, track_id, clip_id)
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
        composition_id: Uuid,
        track_id: Uuid,
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
            .get_clip_mut(composition_id, track_id, clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        clip.update_effect_property(effect_index, property_key, time, value, easing)
            .map_err(|e| LibraryError::Project(e.to_string()))
    }
    pub fn update_style_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
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
            .get_clip_mut(composition_id, track_id, clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip with ID {} not found", clip_id)))?;

        clip.update_style_property(style_index, property_key, time, value, easing)
            .map_err(|e| LibraryError::Project(e.to_string()))
    }

    pub fn set_style_property_attribute(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
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
            .get_clip_mut(composition_id, track_id, clip_id)
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
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(composition_id, track_id, clip_id)
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
        composition_id: Uuid,
        track_id: Uuid,
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
            .get_clip_mut(composition_id, track_id, clip_id)
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
