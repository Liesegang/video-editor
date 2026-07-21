//! Typed Timeline/Inspector facade over the authoritative Project graph.
//!
//! These methods deliberately delegate one-for-one to `ProjectManager` so the
//! application cannot bypass its validation and atomic graph transactions.

use uuid::Uuid;

use super::EditorService;
use crate::animation::EasingFunction;
use crate::editor::project_service::{
    SemanticContainerPropertyProjection, SemanticContainerPropertyStack, SemanticDecoratorStack,
    SemanticEffectStack, SemanticStyleStack,
};
use crate::error::LibraryError;
use crate::model::project::{NodeContainer, PortAddress};
use crate::model::property::{KeyframeId, KeyframeUpdate, Property, PropertyValue};

impl EditorService {
    pub fn semantic_container_property_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticContainerPropertyStack, LibraryError> {
        self.project_manager
            .semantic_container_property_stack(owner)
    }

    pub fn semantic_container_property_projection(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticContainerPropertyProjection, LibraryError> {
        self.project_manager
            .semantic_container_property_projection(owner)
    }

    pub fn update_semantic_container_property_or_keyframe(
        &self,
        owner: NodeContainer,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_semantic_container_property_or_keyframe(
                owner,
                property_key,
                time,
                value,
                easing,
            )
    }

    pub fn replace_semantic_container_property(
        &self,
        owner: NodeContainer,
        property_key: &str,
        property: Property,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .replace_semantic_container_property(owner, property_key, property)
    }

    pub fn set_semantic_container_property_attribute(
        &self,
        owner: NodeContainer,
        property_key: &str,
        attribute_key: String,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .set_semantic_container_property_attribute(
                owner,
                property_key,
                attribute_key,
                attribute_value,
            )
    }

    pub fn add_semantic_container_keyframe(
        &self,
        owner: NodeContainer,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<KeyframeId, LibraryError> {
        self.project_manager.add_semantic_container_keyframe(
            owner,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_semantic_container_keyframe_by_id(
        &self,
        owner: NodeContainer,
        property_key: &str,
        keyframe_id: KeyframeId,
        update: KeyframeUpdate,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_semantic_container_keyframe_by_id(owner, property_key, keyframe_id, update)
    }

    pub fn remove_semantic_container_keyframe_by_id(
        &self,
        owner: NodeContainer,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_semantic_container_keyframe_by_id(owner, property_key, keyframe_id)
    }

    pub fn ensure_semantic_container_transform(
        &self,
        owner: NodeContainer,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .ensure_semantic_container_transform(owner)
    }

    pub fn semantic_container_effect_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticEffectStack, LibraryError> {
        self.project_manager.semantic_container_effect_stack(owner)
    }

    pub fn append_semantic_container_effect(
        &self,
        owner: NodeContainer,
        effect_type: &str,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .append_semantic_container_effect(owner, effect_type)
    }

    pub fn reorder_semantic_container_effects(
        &self,
        owner: NodeContainer,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        self.project_manager
            .reorder_semantic_container_effects(owner, requested)
    }

    pub fn remove_semantic_container_effect(
        &self,
        owner: NodeContainer,
        effect_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_semantic_container_effect(owner, effect_id)
    }

    pub fn semantic_container_style_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticStyleStack, LibraryError> {
        self.project_manager.semantic_container_style_stack(owner)
    }

    pub fn append_semantic_container_style(
        &self,
        owner: NodeContainer,
        style_type: &str,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .append_semantic_container_style(owner, style_type)
    }

    pub fn append_semantic_container_style_after(
        &self,
        owner: NodeContainer,
        style_type: &str,
        after_style_id: Option<Uuid>,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager.append_semantic_container_style_after(
            owner,
            style_type,
            after_style_id,
        )
    }

    pub fn append_semantic_container_style_from_shape(
        &self,
        owner: NodeContainer,
        style_type: &str,
        shape_source: PortAddress,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .append_semantic_container_style_from_shape(owner, style_type, shape_source)
    }

    pub fn reorder_semantic_container_styles(
        &self,
        owner: NodeContainer,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        self.project_manager
            .reorder_semantic_container_styles(owner, requested)
    }

    pub fn remove_semantic_container_style(
        &self,
        owner: NodeContainer,
        style_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_semantic_container_style(owner, style_id)
    }

    pub fn semantic_container_decorator_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticDecoratorStack, LibraryError> {
        self.project_manager
            .semantic_container_decorator_stack(owner)
    }

    pub fn append_semantic_container_decorator(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .append_semantic_container_decorator(owner, decorator_type)
    }

    pub fn append_semantic_container_decorator_for_style(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
        style_anchor_id: Uuid,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .append_semantic_container_decorator_for_style(owner, decorator_type, style_anchor_id)
    }

    pub fn append_semantic_container_decorator_after(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
        decorator_anchor_id: Uuid,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .append_semantic_container_decorator_after(owner, decorator_type, decorator_anchor_id)
    }

    pub fn reorder_semantic_container_decorators(
        &self,
        owner: NodeContainer,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        self.project_manager
            .reorder_semantic_container_decorators(owner, requested)
    }

    pub fn reorder_semantic_container_decorators_for_style(
        &self,
        owner: NodeContainer,
        style_anchor_id: Uuid,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        self.project_manager
            .reorder_semantic_container_decorators_for_style(owner, style_anchor_id, requested)
    }

    pub fn remove_semantic_container_decorator(
        &self,
        owner: NodeContainer,
        decorator_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_semantic_container_decorator(owner, decorator_id)
    }

    pub fn remove_semantic_container_decorator_for_style(
        &self,
        owner: NodeContainer,
        style_anchor_id: Uuid,
        decorator_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_semantic_container_decorator_for_style(owner, style_anchor_id, decorator_id)
    }
}
