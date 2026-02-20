use crate::error::LibraryError;

use crate::model::project::project::Project;
use crate::model::project::property::PropertyValue;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct KeyframeHandler;

impl KeyframeHandler {
    /// Unified method to add a keyframe to any target (Clip, Effect, Style, Effector, Decorator)
    pub fn add_keyframe(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project(format!("Clip {} not found", clip_id)))?;

        // Use the unified accessor
        let prop_map = clip.get_property_map_mut(target.clone()).ok_or_else(|| {
            LibraryError::project(format!("Target {:?} not found in clip {}", target, clip_id))
        })?;

        let prop = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::project(format!("Property {} not found", property_key)))?;

        // Use upsert_keyframe which correctly converts constant → keyframe
        if !prop.upsert_keyframe(time, value, easing) {
            return Err(LibraryError::project(format!(
                "Cannot add keyframe to property {} (evaluator: {})",
                property_key, prop.evaluator
            )));
        }

        Ok(())
    }

    /// Unified method to update a keyframe by index for any target
    pub fn update_keyframe_by_index(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project(format!("Clip {} not found", clip_id)))?;

        let prop_map = clip
            .get_property_map_mut(target.clone())
            .ok_or_else(|| LibraryError::project(format!("Target {:?} not found", target)))?;

        let property = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::project(format!("Property {} not found", property_key)))?;

        // Logic centralized in Property? Or keep here?
        // Property::update_keyframe_at_index is available
        if !property.update_keyframe_at_index(keyframe_index, new_time, new_value, new_easing) {
            return Err(LibraryError::project(format!(
                "Failed to update keyframe at index {} for property {}",
                keyframe_index, property_key
            )));
        }

        Ok(())
    }

    /// Unified method to remove a keyframe by index for any target
    pub fn remove_keyframe_by_index(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project(format!("Clip {} not found", clip_id)))?;

        let prop_map = clip
            .get_property_map_mut(target.clone())
            .ok_or_else(|| LibraryError::project(format!("Target {:?} not found", target)))?;

        let property = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::project(format!("Property {} not found", property_key)))?;

        // Property::remove_keyframe_at_index is available
        if !property.remove_keyframe_at_index(keyframe_index) {
            return Err(LibraryError::project(format!(
                "Failed to remove keyframe at index {} for property {}",
                keyframe_index, property_key
            )));
        }

        Ok(())
    }
}
