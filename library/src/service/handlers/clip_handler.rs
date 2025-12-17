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
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;
        let track = composition.get_track_mut(track_id).ok_or_else(|| {
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

        track.clips.push(final_clip);
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

        if let Some(index) = track.clips.iter().position(|e| e.id == clip_id) {
            track.clips.remove(index);
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
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;
        let track = composition.get_track_mut(track_id).ok_or_else(|| {
            LibraryError::Project(format!(
                "Track with ID {} not found in Composition {}",
                track_id, composition_id
            ))
        })?;

        if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
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
                        clip.source_begin_frame = n.into_inner().round() as u64;
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
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;
        let track = composition.get_track_mut(track_id).ok_or_else(|| {
            LibraryError::Project(format!(
                "Track with ID {} not found in Composition {}",
                track_id, composition_id
            ))
        })?;

        if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
            // Get or create property
            if let Some(prop) = clip.properties.get_mut(property_key) {
                // Check logic: if currently "constant", convert to "keyframe"
                if prop.evaluator == "constant" {
                    // Current value becomes a keyframe at time 0
                    let initial_val = prop
                        .properties
                        .get("value")
                        .cloned()
                        .unwrap_or(PropertyValue::Number(OrderedFloat(0.0)));
                    let kf0 = Keyframe {
                        time: OrderedFloat(0.0),
                        value: initial_val,
                        easing: crate::animation::EasingFunction::Linear,
                    };

                    // New keyframe
                    let kf_new = Keyframe {
                        time: OrderedFloat(time),
                        value: value.clone(),
                        easing: easing.unwrap_or(crate::animation::EasingFunction::Linear),
                    };

                    let keyframes = vec![kf0, kf_new];
                    // Replace property with new Keyframe property
                    *prop = Property::keyframe(keyframes);
                } else if prop.evaluator == "keyframe" {
                    let mut current_keyframes = prop.keyframes();

                    // Check for collision to preserve easing
                    let mut preserved_easing = crate::animation::EasingFunction::Linear;
                    if let Some(idx) = current_keyframes
                        .iter()
                        .position(|k| (k.time.into_inner() - time).abs() < 0.001)
                    {
                        preserved_easing = current_keyframes[idx].easing.clone();
                        current_keyframes.remove(idx);
                    }

                    let final_easing = easing.unwrap_or(preserved_easing);

                    current_keyframes.push(Keyframe {
                        time: OrderedFloat(time),
                        value: value.clone(),
                        easing: final_easing,
                    });

                    // Sort by time
                    current_keyframes.sort_by(|a, b| a.time.cmp(&b.time));

                    *prop = Property::keyframe(current_keyframes);
                }
                // If constant and stays constant (not implemented here? update_property handles that? No, update_property handles simple updates)
                // Wait, update_property_or_keyframe usually handles setting constant too if not keyframe?
                // In ProjectService L691, if evaluator != "keyframe" (and != constant logic i.e. it falls through), it sets as constant.
                // My extracted logic covered constant->keyframe and keyframe->keyframe.
                // It misses the "Stay Constant" update?
                // Ah, in ProjectService L659, it checks `if evaluator == "keyframe"`.
                // `else` (L690): Simply overwrite or create as constant.
                // My copied logic in `add_keyframe` missed the `else` block for simple constant update!
                // I need to add that.
                else {
                    clip.properties
                        .set(property_key.to_string(), Property::constant(value));
                }
            } else {
                // Property doesn't exist, create as constant
                clip.properties
                    .set(property_key.to_string(), Property::constant(value));
            }
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Clip with ID {} not found",
                clip_id
            )))
        }
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
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;
        let track = composition.get_track_mut(track_id).ok_or_else(|| {
            LibraryError::Project(format!(
                "Track with ID {} not found in Composition {}",
                track_id, composition_id
            ))
        })?;

        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
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
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;
        let track = composition.get_track_mut(track_id).ok_or_else(|| {
            LibraryError::Project(format!(
                "Track with ID {} not found in Composition {}",
                track_id, composition_id
            ))
        })?;

        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
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
                    for clip in &track.clips {
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
}
