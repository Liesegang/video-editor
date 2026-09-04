//! Stable-identity keyframe creation, batch update, and removal commands.

use super::lifecycle::ProjectManager;
use crate::editor::handlers;
use crate::editor::handlers::property_ops::PropertyOwner;
use crate::error::LibraryError;
use crate::model::property::{KeyframeId, KeyframeUpdate, PropertyValue};

impl ProjectManager {
    pub fn update_keyframe_by_id(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        keyframe_id: KeyframeId,
        update: KeyframeUpdate,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_keyframe_by_id(
            &self.project,
            owner,
            property_key,
            keyframe_id,
            update,
        )
    }

    pub fn update_keyframes_batch(
        &self,
        updates: &[handlers::keyframe_handler::KeyframeBatchUpdate],
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_keyframes_batch(&self.project, updates)
    }

    pub fn remove_keyframe_by_id(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::remove_keyframe_by_id(
            &self.project,
            owner,
            property_key,
            keyframe_id,
        )
    }

    pub fn add_keyframe(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::add_keyframe(
            &self.project,
            owner,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn add_keyframe_with_id(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<KeyframeId, LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::add_keyframe_with_id(
            &self.project,
            owner,
            property_key,
            time,
            value,
            easing,
        )
    }
}
