//! Shared handler for processing PropertyAction events in the inspector panel.
//! This module reduces duplication across mod.rs, effects.rs, and styles.rs.

// Action handler is now actively used by mod.rs, effects.rs, and styles.rs

use super::properties::PropertyAction;
use crate::command::history::HistoryManager;
use library::project::property::{Property, PropertyValue};
use library::EditorService;
use uuid::Uuid;

pub(super) use library::project::property::PropertyTarget;

/// Context for handling property actions.
pub(super) struct ActionContext<'a> {
    project_service: &'a mut EditorService,
    history_manager: &'a mut HistoryManager,
    clip_id: Uuid,
    current_time: f64,
}

impl<'a> ActionContext<'a> {
    /// Create a new ActionContext.
    pub(super) fn new(
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
    fn handle_update(
        &mut self,
        target: PropertyTarget,
        name: &str,
        value: PropertyValue,
        _get_property: impl Fn(&str) -> Option<library::project::property::Property>,
    ) -> bool {
        let result = self.project_service.update_target_property_or_keyframe(
            self.clip_id,
            target,
            name,
            self.current_time,
            value,
            None,
        );

        if let Err(e) = result {
            log::error!("Failed to update property {}: {:?}", name, e);
        }
        true
    }

    /// Handle a Commit action - saves the current project state to history.
    fn handle_commit(&mut self) {
        let current_state = self.project_service.with_project(|p| p.clone());
        self.history_manager.push_project_state(current_state);
    }

    /// Handle a ToggleKeyframe action - adds or removes a keyframe at current time.
    fn handle_toggle_keyframe(
        &mut self,
        target: PropertyTarget,
        name: &str,
        value: PropertyValue,
        get_property: impl Fn(&str) -> Option<library::project::property::Property>,
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
            self.project_service
                .remove_target_keyframe_by_index(self.clip_id, target, name, index)
        } else {
            self.project_service.add_target_keyframe(
                self.clip_id,
                target,
                name,
                self.current_time,
                value,
                None,
            )
        };

        if let Err(e) = result {
            log::error!("Failed to toggle keyframe for {}: {:?}", name, e);
        }
        true
    }

    /// Handle a SetAttribute action - sets a property attribute.
    fn handle_set_attribute(
        &mut self,
        target: PropertyTarget,
        name: &str,
        attr_key: &str,
        attr_val: PropertyValue,
    ) -> bool {
        let result = self.project_service.set_property_attribute(
            self.clip_id,
            target,
            name,
            attr_key,
            attr_val,
        );

        if let Err(e) = result {
            log::error!("Failed to set attribute {} for {}: {:?}", attr_key, name, e);
        }
        true
    }

    /// Process a list of PropertyActions, handling updates and history commits.
    pub(super) fn handle_actions(
        &mut self,
        actions: Vec<PropertyAction>,
        target: PropertyTarget,
        get_property: impl Fn(&str) -> Option<Property>,
    ) -> bool {
        let mut needs_refresh = false;
        for action in actions {
            match action {
                PropertyAction::Update(name, val) => {
                    self.handle_update(target, &name, val, &get_property);
                    needs_refresh = true;
                }
                PropertyAction::Commit => {
                    self.handle_commit();
                }
                PropertyAction::ToggleKeyframe(name, val) => {
                    self.handle_toggle_keyframe(target, &name, val, &get_property);
                    needs_refresh = true;
                }
                PropertyAction::SetAttribute(name, key, val) => {
                    self.handle_set_attribute(target, &name, &key, val);
                    needs_refresh = true;
                }
            }
        }
        needs_refresh
    }
}
