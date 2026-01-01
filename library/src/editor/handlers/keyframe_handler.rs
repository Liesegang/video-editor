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
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        // Use the unified accessor
        let prop_map = clip.get_property_map_mut(target.clone()).ok_or_else(|| {
            LibraryError::Project(format!("Target {:?} not found in clip {}", target, clip_id))
        })?;

        // Update logic centralized in PropertyMap
        prop_map.update_property_or_keyframe(property_key, time, value, easing);

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
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let prop_map = clip
            .get_property_map_mut(target.clone())
            .ok_or_else(|| LibraryError::Project(format!("Target {:?} not found", target)))?;

        let property = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

        // Logic centralized in Property? Or keep here?
        // Property::update_keyframe_at_index is available
        if !property.update_keyframe_at_index(keyframe_index, new_time, new_value, new_easing) {
            return Err(LibraryError::Project(format!(
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
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let prop_map = clip
            .get_property_map_mut(target.clone())
            .ok_or_else(|| LibraryError::Project(format!("Target {:?} not found", target)))?;

        let property = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

        // Property::remove_keyframe_at_index is available
        if !property.remove_keyframe_at_index(keyframe_index) {
            return Err(LibraryError::Project(format!(
                "Failed to remove keyframe at index {} for property {}",
                keyframe_index, property_key
            )));
        }

        Ok(())
    }
}
