use crate::error::LibraryError;

use crate::model::project::project::Project;
use crate::model::project::property::{Keyframe, Property, PropertyValue};
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct KeyframeHandler;

impl KeyframeHandler {
    pub fn add_keyframe(
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
        let comp = proj
            .get_composition_mut(composition_id)
            .ok_or_else(|| LibraryError::Project(format!("Comp {} not found", composition_id)))?;
        let track = comp
            .get_track_mut(track_id)
            .ok_or_else(|| LibraryError::Project(format!("Track {} not found", track_id)))?;
        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        // Find property or create
        if let Some(prop) = clip.properties.get_mut(property_key) {
            // If constant, convert to keyframe
            if prop.evaluator == "constant" {
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
                let kf_new = Keyframe {
                    time: OrderedFloat(time),
                    value: value,
                    easing: easing.unwrap_or(crate::animation::EasingFunction::Linear),
                };
                *prop = Property::keyframe(vec![kf0, kf_new]);
            } else if prop.evaluator == "keyframe" {
                // Add to list
                // Check if keyframe exists at time?
                let mut kfs = prop.keyframes();
                // Update existing if exists
                if let Some(existing_idx) = kfs
                    .iter()
                    .position(|k| (k.time.into_inner() - time).abs() < 0.001)
                {
                    kfs[existing_idx].value = value;
                    if let Some(e) = easing {
                        kfs[existing_idx].easing = e;
                    }
                } else {
                    kfs.push(Keyframe {
                        time: OrderedFloat(time),
                        value: value,
                        easing: easing.unwrap_or(crate::animation::EasingFunction::Linear),
                    });
                    kfs.sort_by_key(|k| k.time);
                }
                *prop = Property::keyframe(kfs);
            } else {
                // Other evaluator type
                return Err(LibraryError::Project(format!(
                    "Property {} is type {}, cannot add keyframe",
                    property_key, prop.evaluator
                )));
            }
        } else {
            // Create New Keyframe Property implies base value 0? Or use this value?
            // Usually we create constant first. But here we can create keyframe prop directly.
            // But we might need two keyframes (one at 0, one at time)? Or just one?
            // If just one, it's effectively constant.
            // Let's create keyframe prop with this single keyframe.
            let kf = Keyframe {
                time: OrderedFloat(time),
                value,
                easing: easing.unwrap_or(crate::animation::EasingFunction::Linear),
            };
            clip.properties
                .set(property_key.to_string(), Property::keyframe(vec![kf]));
        }
        Ok(())
    }

    pub fn update_keyframe(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
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
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Clip with ID {} not found in Track {}",
                    clip_id, track_id
                ))
            })?;

        let property = clip.properties.get_mut(property_key).ok_or_else(|| {
            LibraryError::Project(format!(
                "Property {} not found in Clip {}",
                property_key, clip_id
            ))
        })?;

        // Expect keyframe evaluator
        if property.evaluator != "keyframe" {
            // Convert to keyframe if constant?
            // For now, assume it's already keyframe or error.
            // Or better, logic to convert. But simpler to error for now.
            return Err(LibraryError::Project(format!(
                "Property {} is not a keyframe property",
                property_key
            )));
        }

        let keyframes_val = property
            .properties
            .get_mut("keyframes")
            .ok_or_else(|| LibraryError::Project("Keyframes array missing".to_string()))?;

        let mut keyframes: Vec<Keyframe> = match keyframes_val {
            PropertyValue::Array(arr) => arr
                .iter()
                .filter_map(|v| serde_json::from_value(serde_json::Value::from(v)).ok())
                .collect(),
            _ => {
                return Err(LibraryError::Project(
                    "Keyframes property is not an array".to_string(),
                ));
            }
        };

        if index >= keyframes.len() {
            return Err(LibraryError::Project(format!(
                "Keyframe index {} out of bounds",
                index
            )));
        }

        let kf = &mut keyframes[index];
        if let Some(t) = new_time {
            kf.time = OrderedFloat(t);
        }
        if let Some(v) = new_value {
            kf.value = v;
        }
        if let Some(e) = new_easing {
            kf.easing = e;
        }

        // Sort by time?
        keyframes.sort_by_key(|k| k.time);

        // Write back
        let new_arr = PropertyValue::Array(
            keyframes
                .into_iter()
                .filter_map(|k| serde_json::to_value(k).ok())
                .map(PropertyValue::from)
                .collect(),
        );
        *keyframes_val = new_arr;

        Ok(())
    }

    pub fn update_effect_keyframe_by_index(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
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
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Clip with ID {} not found in Track {}",
                    clip_id, track_id
                ))
            })?;

        // Find effect by index (in effects list property)
        // This is tricky. Effect instances are usually in "effects" property which is array of PropertyValue (Map?)
        // Wait, how are effects stored?
        // In `ProjectService`, effects were likely managed via plugins.
        // Assuming `effects` is a property on clip.

        let effects_prop = clip
            .properties
            .get_mut("effects")
            .ok_or_else(|| LibraryError::Project("Effects property not found".to_string()))?;

        let effects_arr = match effects_prop.properties.get_mut("value") {
            // Or generic logic
            Some(PropertyValue::Array(arr)) => arr,
            _ => {
                return Err(LibraryError::Project(
                    "Effects value is not an array".to_string(),
                ));
            }
        };

        if effect_index >= effects_arr.len() {
            return Err(LibraryError::Project(format!(
                "Effect index {} out of bounds",
                effect_index
            )));
        }

        let effect_val = &mut effects_arr[effect_index];
        // effect_val is PropertyValue::Map or similar representing the effect instance
        let effect_map_val = match effect_val {
            PropertyValue::Map(m) => m,
            _ => {
                return Err(LibraryError::Project(
                    "Effect item is not a map".to_string(),
                ));
            }
        };

        // Inside effect map, there is "properties" map? Or just keys?
        // Effect instance structure: { "id": "...", "properties": { "prop_key": Property } }
        // Inspecting `plugin/mod.rs`, EffectPlugin params are HashMap<String, PropertyValue>.
        // But stored in Project?

        // Let's assume structure: "properties" -> Map -> "prop_key" -> PropertyValue (which wraps Property?)
        // No, Project structure usually has `Property` struct.
        // If `effect_val` is `PropertyValue`, it cannot contain `Property` struct directly unless serialized.

        // Actually, `update_effect_keyframe_by_index` might need to look at `properties` key inside the effect map.
        let props_map_val = effect_map_val
            .get_mut("properties")
            .ok_or_else(|| LibraryError::Project("Effect properties not found".to_string()))?;
        let props_map = match props_map_val {
            PropertyValue::Map(m) => m,
            _ => {
                return Err(LibraryError::Project(
                    "Effect properties is not a map".to_string(),
                ));
            }
        };

        let target_prop_val = props_map.get_mut(property_key).ok_or_else(|| {
            LibraryError::Project(format!("Effect property {} not found", property_key))
        })?;

        // target_prop_val is PropertyValue representing a Property?
        // Wait, PropertyValue cannot represent Property (struct with evaluator, properties).
        // It represents the VALUE.
        // Unless we changed how effects are stored.

        // If effects are stored as PropertyValue::Map, they just have values.
        // But we want to Keyframe them.
        // This means the Effect storage model in `Project` must support `Property` structs.
        // But `clip.properties` is `HashMap<String, Property>`.
        // `effects` is a `Property`. Its value is `Array` of `Map`.
        // Inside that `Map`, if we have "properties", they are `PropertyValue`.
        // `PropertyValue` is Enum.
        // It CANNOT hold `Property`.

        // CONCERN: Effect properties were maybe recursed?
        // Or maybe `effects` list contains IDs, and properties are stored elsewhere?

        // Re-reading `ProjectManager.rs` line 486 (Step 1755):
        // `PropertyValue::Array(vec![PropertyValue::from(style_json)])`
        // StyleConfig has `id` and `style`.
        // Effect likely similar.

        // If Effect properties are simple PropertyValues, they CANNOT be keyframed (which requires `Property` struct).
        // UNLESS the `Property` struct is embedded in `PropertyValue::Map`???
        // `Property` has `evaluator`.
        // `PropertyValue::Map` has keys.

        // If the architecture allows keyframing effects, then `Effect` structure must allow it.
        // Maybe `update_effect_keyframe_by_index` fails if it's not supported?

        // For now, I will assume `target_prop_val` IS the `Property` serialized as `PropertyValue::Map`.
        // (evaluator, properties).

        // Check if it has "evaluator" key.
        if let PropertyValue::Map(pmap) = target_prop_val {
            // It's a Property object serialized as Value.
            // We need to mutate it.
            // Access "properties" -> "keyframes".

            let kfs_prop_val = pmap.get_mut("properties").and_then(|p| match p {
                PropertyValue::Map(m) => m.get_mut("keyframes"),
                _ => None,
            });

            if let Some(PropertyValue::Array(kfs_arr)) = kfs_prop_val {
                // LOGIC SAME AS ABOVE
                let mut keyframes: Vec<Keyframe> = kfs_arr
                    .iter()
                    .filter_map(|v| serde_json::from_value(serde_json::Value::from(v)).ok())
                    .collect();
                if keyframe_index < keyframes.len() {
                    let kf = &mut keyframes[keyframe_index];
                    if let Some(t) = new_time {
                        kf.time = OrderedFloat(t);
                    }
                    if let Some(v) = new_value {
                        kf.value = v;
                    }
                    if let Some(e) = new_easing {
                        kf.easing = e;
                    }
                }
                keyframes.sort_by_key(|k| k.time);

                // Write back
                let new_arr = PropertyValue::Array(
                    keyframes
                        .into_iter()
                        .filter_map(|k| serde_json::to_value(k).ok())
                        .map(PropertyValue::from)
                        .collect(),
                );
                // We need to re-find it to assign because of borrow checker if I didn't split well.
                // But here I have mutable ref `target_prop_val`.
                if let PropertyValue::Map(pmap2) = target_prop_val {
                    if let Some(PropertyValue::Map(props2)) = pmap2.get_mut("properties") {
                        props2.insert("keyframes".to_string(), new_arr);
                    }
                }
            } else {
                return Err(LibraryError::Project(
                    "Keyframes not found in effect property".to_string(),
                ));
            }
        } else {
            return Err(LibraryError::Project(
                "Effect property is not a map".to_string(),
            ));
        }

        Ok(())
    }

    pub fn remove_effect_keyframe_by_index(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        let mut project_write = project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;

        let comp = project_write
            .compositions
            .iter_mut()
            .find(|c| c.id == composition_id)
            .ok_or_else(|| {
                LibraryError::Project(format!("Composition {} not found", composition_id))
            })?;

        let track = comp
            .tracks
            .iter_mut()
            .find(|t| t.id == track_id)
            .ok_or_else(|| LibraryError::Project(format!("Track {} not found", track_id)))?;

        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let effect_map_val = clip.effects.get_mut(effect_index).ok_or_else(|| {
            LibraryError::Project(format!("Effect index {} out of bounds", effect_index))
        })?;

        // effect_map_val is likely &mut EffectConfig which has .properties field (HashMap)
        let target_prop_val = effect_map_val
            .properties
            .get_mut(property_key)
            .ok_or_else(|| {
                LibraryError::Project(format!("Effect property {} not found", property_key))
            })?;

        let kfs_prop_val = target_prop_val
            .properties
            .get_mut("keyframes")
            .ok_or_else(|| {
                LibraryError::Project("Keyframes not found in effect property".to_string())
            })?;

        if let PropertyValue::Array(kfs_arr) = kfs_prop_val {
            if keyframe_index >= kfs_arr.len() {
                return Err(LibraryError::Project(format!(
                    "Keyframe index {} out of bounds for removal",
                    keyframe_index
                )));
            }
            kfs_arr.remove(keyframe_index);
        } else {
            return Err(LibraryError::Project(
                "Keyframes not found in effect property".to_string(),
            ));
        }

        Ok(())
    }
    pub fn add_effect_keyframe(
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
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;
        let comp = proj
            .get_composition_mut(composition_id)
            .ok_or_else(|| LibraryError::Project("Comp not found".to_string()))?;
        let track = comp
            .get_track_mut(track_id)
            .ok_or_else(|| LibraryError::Project("Track not found".to_string()))?;
        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        let effect_map_val = clip
            .effects
            .get_mut(effect_index)
            .ok_or_else(|| LibraryError::Project("Effect index out of bounds".to_string()))?;
        let target_prop_val = effect_map_val
            .properties
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project("Effect property not found".to_string()))?;

        // Logic: Promotes to keyframe if constant, or adds if keyframe.
        // Note: previous logic accessed "value" key inside properties if generic.
        // But `target_prop_val` IS the Property object (struct).
        // So we can use helper logic. (Ideally extracted).

        // Check evaluator.
        // Since target_prop_val is &mut Property, we can check .evaluator
        // But Property struct fields are public? Yes.

        if target_prop_val.evaluator == "constant" {
            let initial_val = target_prop_val
                .properties
                .get("value")
                .cloned()
                .unwrap_or(PropertyValue::Number(OrderedFloat(0.0)));
            let kf0 = Keyframe {
                time: OrderedFloat(0.0),
                value: initial_val,
                easing: crate::animation::EasingFunction::Linear,
            };
            let kf_new = Keyframe {
                time: OrderedFloat(time),
                value: value,
                easing: easing.unwrap_or_default(),
            };
            *target_prop_val = Property::keyframe(vec![kf0, kf_new]);
        } else if target_prop_val.evaluator == "keyframe" {
            let mut kfs = target_prop_val.keyframes();
            if let Some(idx) = kfs
                .iter()
                .position(|k| (k.time.into_inner() - time).abs() < 0.001)
            {
                kfs[idx].value = value;
                if let Some(e) = easing {
                    kfs[idx].easing = e;
                }
            } else {
                kfs.push(Keyframe {
                    time: OrderedFloat(time),
                    value: value,
                    easing: easing.unwrap_or_default(),
                });
                kfs.sort_by_key(|k| k.time);
            }
            *target_prop_val = Property::keyframe(kfs);
        } else {
            return Err(LibraryError::Project(
                "Cannot keyframe this property type".to_string(),
            ));
        }
        Ok(())
    }

    pub fn add_style_keyframe(
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
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;
        let comp = proj
            .get_composition_mut(composition_id)
            .ok_or_else(|| LibraryError::Project("Comp not found".to_string()))?;
        let track = comp
            .get_track_mut(track_id)
            .ok_or_else(|| LibraryError::Project("Track not found".to_string()))?;
        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        let style = clip
            .styles
            .get_mut(style_index)
            .ok_or_else(|| LibraryError::Project("Style index out of bounds".to_string()))?;
        let prop = style
            .properties
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project("Style property not found".to_string()))?;

        if prop.evaluator == "constant" {
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
            let kf_new = Keyframe {
                time: OrderedFloat(time),
                value: value,
                easing: easing.unwrap_or_default(),
            };
            *prop = Property::keyframe(vec![kf0, kf_new]);
        } else if prop.evaluator == "keyframe" {
            let mut kfs = prop.keyframes();
            if let Some(idx) = kfs
                .iter()
                .position(|k| (k.time.into_inner() - time).abs() < 0.001)
            {
                kfs[idx].value = value;
                if let Some(e) = easing {
                    kfs[idx].easing = e;
                }
            } else {
                kfs.push(Keyframe {
                    time: OrderedFloat(time),
                    value: value,
                    easing: easing.unwrap_or_default(),
                });
                kfs.sort_by_key(|k| k.time);
            }
            *prop = Property::keyframe(kfs);
        } else {
            return Err(LibraryError::Project(
                "Cannot keyframe this property type".to_string(),
            ));
        }
        Ok(())
    }

    pub fn remove_style_keyframe(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;
        let comp = proj
            .get_composition_mut(composition_id)
            .ok_or_else(|| LibraryError::Project("Comp not found".to_string()))?;
        let track = comp
            .get_track_mut(track_id)
            .ok_or_else(|| LibraryError::Project("Track not found".to_string()))?;
        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        let style = clip
            .styles
            .get_mut(style_index)
            .ok_or_else(|| LibraryError::Project("Style index out of bounds".to_string()))?;
        let prop = style
            .properties
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project("Style property not found".to_string()))?;

        if prop.evaluator == "keyframe" {
            let mut kfs = prop.keyframes();
            if keyframe_index < kfs.len() {
                kfs.remove(keyframe_index);
                *prop = Property::keyframe(kfs);
            }
        }
        Ok(())
    }
    pub fn update_style_keyframe_by_index(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;
        let comp = proj
            .get_composition_mut(composition_id)
            .ok_or_else(|| LibraryError::Project("Comp not found".to_string()))?;
        let track = comp
            .get_track_mut(track_id)
            .ok_or_else(|| LibraryError::Project("Track not found".to_string()))?;
        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        let style = clip
            .styles
            .get_mut(style_index)
            .ok_or_else(|| LibraryError::Project("Style index out of bounds".to_string()))?;
        let prop = style
            .properties
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project("Style property not found".to_string()))?;

        if prop.evaluator == "keyframe" {
            let mut kfs = prop.keyframes();
            if keyframe_index < kfs.len() {
                let kf = &mut kfs[keyframe_index];
                if let Some(t) = new_time {
                    kf.time = OrderedFloat(t);
                }
                if let Some(v) = new_value {
                    kf.value = v;
                }
                if let Some(e) = new_easing {
                    kf.easing = e;
                }
            } else {
                 return Err(LibraryError::Project(format!(
                    "Keyframe index {} out of bounds",
                    keyframe_index
                )));
            }
            kfs.sort_by_key(|k| k.time);
            *prop = Property::keyframe(kfs);
        } else {
            return Err(LibraryError::Project(
                "Property is not a keyframe property".to_string(),
            ));
        }
        Ok(())
    }
}

