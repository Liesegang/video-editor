//! Shared handler for processing PropertyAction events in the inspector panel.
//! This module reduces duplication across mod.rs, effects.rs, and styles.rs.

// Action handler is now actively used by mod.rs, effects.rs, and styles.rs

use crate::action::HistoryManager;
use crate::ui::panels::inspector::properties::PropertyAction;
use library::model::property::{Property, PropertyValue};
use library::{EditorService, PropertyOwner};

/// Context for handling property actions.
pub struct ActionContext<'a> {
    pub project_service: &'a mut EditorService,
    pub history_manager: &'a mut HistoryManager,
    pub owner: PropertyOwner,
    pub current_time: f64,
}

impl<'a> ActionContext<'a> {
    /// Create a new ActionContext.
    pub fn new(
        project_service: &'a mut EditorService,
        history_manager: &'a mut HistoryManager,
        owner: PropertyOwner,
        current_time: f64,
    ) -> Self {
        Self {
            project_service,
            history_manager,
            owner,
            current_time,
        }
    }

    /// Handle an Update action - updates the property value.
    pub fn handle_update(
        &mut self,
        name: &str,
        value: PropertyValue,
        get_property: impl Fn(&str) -> Option<library::model::property::Property>,
    ) -> bool {
        let _ = get_property;
        let result = self.project_service.update_property_or_keyframe(
            self.owner,
            name,
            self.current_time,
            value,
            None,
        );

        match result {
            Ok(()) => true,
            Err(e) => {
                log::error!("Failed to update property {}: {:?}", name, e);
                false
            }
        }
    }

    /// Handle a Commit action - saves the current project state to history.
    pub fn handle_commit(&mut self) {
        match self.project_service.get_project().read() {
            Ok(project) => self.history_manager.push_project_state(project.clone()),
            Err(error) => log::error!("Failed to capture Inspector history: {error}"),
        }
    }

    /// Handle a ToggleKeyframe action - adds or removes a keyframe at current time.
    pub fn handle_toggle_keyframe(
        &mut self,
        name: &str,
        value: PropertyValue,
        get_property: impl Fn(&str) -> Option<library::model::property::Property>,
    ) -> bool {
        const TOLERANCE: f64 = 0.001;

        // Check if keyframe exists at current time
        let keyframe_id = get_property(name).and_then(|prop| {
            if prop.evaluator == "keyframe" {
                prop.keyframe_id_at(self.current_time, TOLERANCE)
            } else {
                None
            }
        });

        let result = if let Some(keyframe_id) = keyframe_id {
            // Remove existing keyframe
            self.project_service
                .remove_keyframe_by_id(self.owner, name, keyframe_id)
        } else {
            library::editor::handlers::keyframe_handler::KeyframeHandler::add_keyframe(
                &self.project_service.get_project(),
                self.owner,
                name,
                self.current_time,
                value,
                None,
            )
        };

        match result {
            Ok(()) => {
                // Keyframe toggles are atomic button actions and do not emit a
                // separate widget Commit event.
                self.handle_commit();
                true
            }
            Err(e) => {
                log::error!("Failed to toggle keyframe for {}: {:?}", name, e);
                false
            }
        }
    }

    /// Handle a SetAttribute action - sets a property attribute.
    pub fn handle_set_attribute(
        &mut self,
        name: &str,
        attr_key: &str,
        attr_val: PropertyValue,
    ) -> bool {
        let result = self
            .project_service
            .set_property_attribute(self.owner, name, attr_key, attr_val);

        match result {
            Ok(()) => {
                // Attribute dropdowns are atomic and do not emit a separate
                // Commit event after changing the Project.
                self.handle_commit();
                true
            }
            Err(e) => {
                log::error!("Failed to set attribute {} for {}: {:?}", attr_key, name, e);
                false
            }
        }
    }

    /// Process a list of PropertyActions, handling updates and history commits.
    pub fn handle_actions(
        &mut self,
        actions: Vec<PropertyAction>,
        get_property: impl Fn(&str) -> Option<Property>,
    ) -> bool {
        let mut needs_refresh = false;
        for action in actions {
            match action {
                PropertyAction::Update(name, val) => {
                    needs_refresh |= self.handle_update(&name, val, &get_property);
                }
                PropertyAction::Commit => {
                    self.handle_commit();
                }
                PropertyAction::ToggleKeyframe(name, val) => {
                    needs_refresh |= self.handle_toggle_keyframe(&name, val, &get_property);
                }
                PropertyAction::SetAttribute(name, key, val) => {
                    needs_refresh |= self.handle_set_attribute(&name, &key, val);
                }
            }
        }
        needs_refresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::cache::CacheManager;
    use library::model::property::PropertyValue;
    use library::model::{Node, Project};
    use library::plugin::PluginManager;
    use ordered_float::OrderedFloat;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    fn node_property(project: &Project, node_id: Uuid, key: &str) -> Option<Property> {
        project.get_node(node_id)?.properties().get(key).cloned()
    }

    #[test]
    fn inspector_keyframe_crud_edits_an_operation_nodes_direct_properties() {
        let plugins = Arc::new(PluginManager::default());
        let node = plugins.create_effect_operation_node("blur").unwrap();
        let node_id = node.id;

        let mut project = Project::new("inspector keyframes");
        project.add_node(node);
        let project = Arc::new(RwLock::new(project));
        let mut service =
            EditorService::new(Arc::clone(&project), plugins, Arc::new(CacheManager::new()))
                .unwrap();
        let mut history = HistoryManager::new();
        history.push_project_state(project.read().unwrap().clone());

        let initial_depth = history.undo_depth();
        let property = node_property(&project.read().unwrap(), node_id, "sigma_x");
        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Node(node_id),
            2.5,
        );
        assert!(context.handle_toggle_keyframe("sigma_x", number(10.0), |_| { property.clone() }));
        assert_eq!(history.undo_depth(), initial_depth + 1);

        let keyframed = node_property(&project.read().unwrap(), node_id, "sigma_x")
            .expect("operation property should remain present");
        assert_eq!(keyframed.evaluator, "keyframe");
        let keyframe = keyframed.keyframes().into_iter().next().unwrap();
        assert_eq!(keyframe.time, OrderedFloat(2.5));

        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Node(node_id),
            2.5,
        );
        assert!(context.handle_actions(
            vec![
                PropertyAction::Update("sigma_x".to_string(), number(20.0)),
                PropertyAction::Commit,
            ],
            |name| node_property(&project.read().unwrap(), node_id, name),
        ));
        let updated = node_property(&project.read().unwrap(), node_id, "sigma_x").unwrap();
        assert_eq!(
            updated.keyframe_by_id(keyframe.id).unwrap().value,
            number(20.0)
        );

        let property = Some(updated);
        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Node(node_id),
            2.5,
        );
        assert!(context.handle_toggle_keyframe("sigma_x", number(0.0), |_| { property.clone() }));
        let restored = node_property(&project.read().unwrap(), node_id, "sigma_x").unwrap();
        assert_eq!(restored.evaluator, "constant");
        assert_eq!(restored.value(), Some(&number(20.0)));
    }

    #[test]
    fn inspector_materializes_a_missing_property_as_a_typed_keyframe() {
        let node = Node::new_merge("sparse");
        let node_id = node.id;
        let mut initial = Project::new("missing property");
        initial.add_node(node);
        let project = Arc::new(RwLock::new(initial));
        let mut service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )
        .unwrap();
        let mut history = HistoryManager::new();
        history.push_project_state(project.read().unwrap().clone());
        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Node(node_id),
            1.25,
        );

        assert!(context.handle_toggle_keyframe("new_amount", number(7.5), |_| None,));
        let property = node_property(&project.read().unwrap(), node_id, "new_amount").unwrap();
        assert_eq!(property.evaluator, "keyframe");
        assert_eq!(property.keyframes()[0].value, number(7.5));
    }
}
