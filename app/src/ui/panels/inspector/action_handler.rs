//! Shared handler for processing PropertyAction events in the inspector panel.
//! This module reduces duplication across mod.rs, effects.rs, and styles.rs.

// Action handler is now actively used by mod.rs, effects.rs, and styles.rs

use crate::action::HistoryManager;
use library::model::project::property::PropertyValue;
use library::EditorService;
use uuid::Uuid;

pub use library::model::project::property::PropertyTarget;

/// Context for handling property actions.
/// Context for handling property actions.
pub struct ActionContext<'a> {
    pub project_service: &'a mut EditorService,
    pub history_manager: &'a mut HistoryManager,
    pub clip_id: Uuid,
    pub current_time: f64,
}

impl<'a> ActionContext<'a> {
    /// Create a new ActionContext.
    pub fn new(
        project_service: &'a mut EditorService,
        history_manager: &'a mut HistoryManager,
        clip_id: Uuid,
        current_time: f64,
    ) -> Self {
        Self {
            project_service,
            history_manager,
            clip_id,
            current_time,
        }
    }

    /// Handle an Update action - updates the property value.
    pub fn handle_update(
        &mut self,
        target: PropertyTarget,
        name: &str,
        value: PropertyValue,
        get_property: impl Fn(&str) -> Option<library::model::project::property::Property>,
    ) -> bool {
        let is_keyframed = get_property(name)
            .map(|p| p.evaluator == "keyframe")
            .unwrap_or(false);

        let result = match target {
            PropertyTarget::Clip => {
                if is_keyframed {
                    self.project_service.update_property_or_keyframe(
                        self.clip_id,
                        name,
                        self.current_time,
                        value,
                        None,
                    )
                } else {
                    self.project_service
                        .update_clip_property(self.clip_id, name, value)
                }
            }
            PropertyTarget::Effect(idx) => self.project_service.update_effect_property_or_keyframe(
                self.clip_id,
                idx,
                name,
                self.current_time,
                value,
                None,
            ),
            PropertyTarget::Style(idx) => self.project_service.update_style_property_or_keyframe(
                self.clip_id,
                idx,
                name,
                self.current_time,
                value,
                None,
            ),
            PropertyTarget::Effector(idx) => {
                self.project_service.update_effector_property_or_keyframe(
                    self.clip_id,
                    idx,
                    name,
                    self.current_time,
                    value,
                    None,
                )
            }
            PropertyTarget::Decorator(idx) => {
                self.project_service.update_decorator_property_or_keyframe(
                    self.clip_id,
                    idx,
                    name,
                    self.current_time,
                    value,
                    None,
                )
            }
        };

        if let Err(e) = result {
            eprintln!("Failed to update property {}: {:?}", name, e);
        }
        true
    }

    /// Handle a Commit action - saves the current project state to history.
    pub fn handle_commit(&mut self) {
        let current_state = self.project_service.get_project().read().unwrap().clone();
        self.history_manager.push_project_state(current_state);
    }

    /// Handle a ToggleKeyframe action - adds or removes a keyframe at current time.
    pub fn handle_toggle_keyframe(
        &mut self,
        target: PropertyTarget,
        name: &str,
        value: PropertyValue,
        get_property: impl Fn(&str) -> Option<library::model::project::property::Property>,
    ) -> bool {
        const TOLERANCE: f64 = 0.001;

        // Check if keyframe exists at current time
        let keyframe_index = get_property(name).and_then(|prop| {
            if prop.evaluator == "keyframe" {
                prop.keyframe_index_at(self.current_time, TOLERANCE)
            } else {
                None
            }
        });

        let result = if let Some(index) = keyframe_index {
            // Remove existing keyframe
            match target {
                PropertyTarget::Clip => {
                    self.project_service
                        .remove_keyframe(self.clip_id, name, index)
                }
                PropertyTarget::Effect(idx) => self
                    .project_service
                    .remove_effect_keyframe_by_index(self.clip_id, idx, name, index),
                PropertyTarget::Style(idx) => {
                    self.project_service
                        .remove_style_keyframe(self.clip_id, idx, name, index)
                }
                // TODO: Implement remove for Effector/Decorator when needed
                // For now, these are not fully supported or exposed via specialized methods
                // If remove methods are missing, we might need to add them to service first.
                // Assuming property update handles basics, but keyframe removal requires specific methods.
                PropertyTarget::Effector(eff_idx) => self
                    .project_service
                    .remove_effector_keyframe_by_index(self.clip_id, eff_idx, name, index),
                PropertyTarget::Decorator(dec_idx) => self
                    .project_service
                    .remove_decorator_keyframe_by_index(self.clip_id, dec_idx, name, index),
            }
        } else {
            // Add new keyframe
            match target {
                PropertyTarget::Clip => self.project_service.add_keyframe(
                    self.clip_id,
                    name,
                    self.current_time,
                    value,
                    None,
                ),
                PropertyTarget::Effect(idx) => self.project_service.add_effect_keyframe(
                    self.clip_id,
                    idx,
                    name,
                    self.current_time,
                    value,
                    None,
                ),
                PropertyTarget::Style(idx) => self.project_service.add_style_keyframe(
                    self.clip_id,
                    idx,
                    name,
                    self.current_time,
                    value,
                    None,
                ),
                // Using generic update for add_keyframe behavior for now if specific add_keyframe missing?
                // Actually update_..._or_keyframe handles adding if type is keyframe.
                PropertyTarget::Effector(idx) => self.project_service.add_effector_keyframe(
                    self.clip_id,
                    idx,
                    name,
                    self.current_time,
                    value,
                    None,
                ),
                PropertyTarget::Decorator(idx) => self.project_service.add_decorator_keyframe(
                    self.clip_id,
                    idx,
                    name,
                    self.current_time,
                    value,
                    None,
                ),
            }
        };

        if let Err(e) = result {
            eprintln!("Failed to toggle keyframe for {}: {:?}", name, e);
        }
        true
    }

    /// Handle a SetAttribute action - sets a property attribute.
    pub fn handle_set_attribute(
        &mut self,
        target: PropertyTarget,
        name: &str,
        attr_key: &str,
        attr_val: PropertyValue,
    ) -> bool {
        let result = match target {
            PropertyTarget::Clip => self.project_service.set_clip_property_attribute(
                self.clip_id,
                name,
                attr_key,
                attr_val,
            ),
            PropertyTarget::Effect(idx) => self.project_service.set_effect_property_attribute(
                self.clip_id,
                idx,
                name,
                attr_key,
                attr_val,
            ),
            PropertyTarget::Style(idx) => self.project_service.set_style_property_attribute(
                self.clip_id,
                idx,
                name,
                attr_key,
                attr_val,
            ),
            // TODO: Implement for Effector/Decorator
            PropertyTarget::Effector(_) | PropertyTarget::Decorator(_) => Ok(()),
        };

        if let Err(e) = result {
            eprintln!("Failed to set attribute {} for {}: {:?}", attr_key, name, e);
        }
        true
    }
}
