use crate::editor::handlers;
use crate::editor::handlers::property_ops::PropertyOwner;
use crate::error::LibraryError;
use crate::model::property::{Property, PropertyValue};

use super::ProjectManager;

impl ProjectManager {
    pub fn update_property_or_keyframe(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_property_or_keyframe(
            &self.project,
            owner,
            property_key,
            time,
            value,
            easing,
        )
    }

    /// Replaces one authored Property evaluator/value atomically in the
    /// authoritative Project. Inspector mode changes use this instead of
    /// mutating a detached UI copy.
    pub fn replace_property(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        property: Property,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::replace_property(
            &self.project,
            owner,
            property_key,
            property,
        )
    }

    pub fn set_expression_source(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        source: String,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::set_expression_source(
            &self.project,
            owner,
            property_key,
            source,
        )
    }
}
