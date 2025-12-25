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
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        // Get or create property
        if let Some(prop) = clip.properties.get_mut(property_key) {
            if !prop.upsert_keyframe(time, value, easing) {
                return Err(LibraryError::Project(format!(
                    "Property {} is type {}, cannot add keyframe",
                    property_key, prop.evaluator
                )));
            }
        } else {
            // Create new keyframe property
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
        _composition_id: Uuid,
        _track_id: Uuid,
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

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let property = clip.properties.get_mut(property_key).ok_or_else(|| {
            LibraryError::Project(format!(
                "Property {} not found in Clip {}",
                property_key, clip_id
            ))
        })?;

        if property.evaluator != "keyframe" {
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

        keyframes.sort_by_key(|k| k.time);

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
        _composition_id: Uuid,
        _track_id: Uuid,
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

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let effect = clip.effects.get_mut(effect_index).ok_or_else(|| {
            LibraryError::Project(format!("Effect index {} out of bounds", effect_index))
        })?;

        let prop = effect.properties.get_mut(property_key).ok_or_else(|| {
            LibraryError::Project(format!("Effect property {} not found", property_key))
        })?;

        if prop.evaluator != "keyframe" {
            return Err(LibraryError::Project(
                "Effect property is not keyframeable".to_string(),
            ));
        }

        let kfs_val = prop
            .properties
            .get_mut("keyframes")
            .ok_or_else(|| LibraryError::Project("Keyframes not found".to_string()))?;

        if let PropertyValue::Array(kfs_arr) = kfs_val {
            let mut keyframes: Vec<Keyframe> = kfs_arr
                .iter()
                .filter_map(|v| serde_json::from_value(serde_json::Value::from(v)).ok())
                .collect();

            if keyframe_index >= keyframes.len() {
                return Err(LibraryError::Project(format!(
                    "Keyframe index {} out of bounds",
                    keyframe_index
                )));
            }

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

            keyframes.sort_by_key(|k| k.time);

            let new_arr = PropertyValue::Array(
                keyframes
                    .into_iter()
                    .filter_map(|k| serde_json::to_value(k).ok())
                    .map(PropertyValue::from)
                    .collect(),
            );
            *kfs_val = new_arr;
        } else {
            return Err(LibraryError::Project(
                "Keyframes is not an array".to_string(),
            ));
        }

        Ok(())
    }

    pub fn remove_effect_keyframe_by_index(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let effect = clip.effects.get_mut(effect_index).ok_or_else(|| {
            LibraryError::Project(format!("Effect index {} out of bounds", effect_index))
        })?;

        let prop = effect.properties.get_mut(property_key).ok_or_else(|| {
            LibraryError::Project(format!("Effect property {} not found", property_key))
        })?;

        let kfs_val = prop
            .properties
            .get_mut("keyframes")
            .ok_or_else(|| LibraryError::Project("Keyframes not found".to_string()))?;

        if let PropertyValue::Array(kfs_arr) = kfs_val {
            if keyframe_index >= kfs_arr.len() {
                return Err(LibraryError::Project(format!(
                    "Keyframe index {} out of bounds for removal",
                    keyframe_index
                )));
            }
            kfs_arr.remove(keyframe_index);
        } else {
            return Err(LibraryError::Project(
                "Keyframes is not an array".to_string(),
            ));
        }

        Ok(())
    }

    pub fn add_effect_keyframe(
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
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        let effect = clip
            .effects
            .get_mut(effect_index)
            .ok_or_else(|| LibraryError::Project("Effect index out of bounds".to_string()))?;
        let prop = effect
            .properties
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project("Effect property not found".to_string()))?;

        if !prop.upsert_keyframe(time, value, easing) {
            return Err(LibraryError::Project(
                "Cannot keyframe this property type".to_string(),
            ));
        }
        Ok(())
    }

    pub fn add_style_keyframe(
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
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project("Clip not found".to_string()))?;

        let style = clip
            .styles
            .get_mut(style_index)
            .ok_or_else(|| LibraryError::Project("Style index out of bounds".to_string()))?;
        let prop = style
            .properties
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project("Style property not found".to_string()))?;

        if !prop.upsert_keyframe(time, value, easing) {
            return Err(LibraryError::Project(
                "Cannot keyframe this property type".to_string(),
            ));
        }
        Ok(())
    }

    pub fn remove_style_keyframe(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        _track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
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
        _composition_id: Uuid,
        _track_id: Uuid,
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

        let clip = proj
            .get_clip_mut(clip_id)
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
