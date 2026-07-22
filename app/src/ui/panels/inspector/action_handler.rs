//! Shared handler for processing PropertyAction events in the inspector panel.
//! This module reduces duplication across mod.rs, effects.rs, and styles.rs.

// Action handler is now actively used by mod.rs, effects.rs, and styles.rs

use crate::action::HistoryManager;
use crate::ui::panels::inspector::property_authoring::{PropertyAction, PropertyAuthoringMode};
use crate::ui::widgets::property_mode::property_for_mode;
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

    fn handle_set_mode(
        &mut self,
        name: &str,
        mode: PropertyAuthoringMode,
        current_value: PropertyValue,
        get_property: impl Fn(&str) -> Option<Property>,
    ) -> bool {
        let current = get_property(name);
        let replacement =
            match property_for_mode(current.as_ref(), mode, current_value, self.current_time) {
                Ok(property) => property,
                Err(error) => {
                    log::error!("Failed to change property {name} authoring mode: {error}");
                    return false;
                }
            };
        match self
            .project_service
            .replace_property(self.owner, name, replacement)
        {
            Ok(()) => {
                // A mode selector is an atomic action; capture it immediately.
                self.handle_commit();
                true
            }
            Err(error) => {
                log::error!("Failed to replace property {name}: {error}");
                false
            }
        }
    }

    fn handle_expression_source(&mut self, name: &str, source: String) -> bool {
        match self
            .project_service
            .set_expression_source(self.owner, name, source)
        {
            Ok(()) => true,
            Err(error) => {
                log::error!("Failed to update Expression source for {name}: {error}");
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
                PropertyAction::SetMode(name, mode, value) => {
                    needs_refresh |= self.handle_set_mode(&name, mode, value, &get_property);
                }
                PropertyAction::SetExpressionSource(name, source) => {
                    needs_refresh |= self.handle_expression_source(&name, source);
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
    use library::model::{Clip, DataContent, Node, Project};
    use library::plugin::PluginManager;
    use ordered_float::OrderedFloat;
    use std::error::Error;
    use std::io;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    type TestResult = Result<(), Box<dyn Error>>;

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
    fn canonical_color_picker_update_preserves_keyframe_mode_and_one_undo_step() -> TestResult {
        let plugins = Arc::new(PluginManager::default());
        let node = Node::new_data("Color", DataContent::Color);
        let node_id = node.id;
        let initial_value = node
            .properties()
            .get("value")
            .and_then(Property::value)
            .cloned()
            .ok_or_else(|| io::Error::other("Color Node has no initialized value"))?;
        let mut initial = Project::new("canonical color picker history");
        initial.add_node(node);
        let project = Arc::new(RwLock::new(initial));
        let mut service = EditorService::new(
            Arc::clone(&project),
            Arc::clone(&plugins),
            Arc::new(CacheManager::new()),
        )?;
        let mut history = HistoryManager::new();
        history.push_project_state(
            project
                .read()
                .map_err(|_| io::Error::other("Project read lock poisoned"))?
                .clone(),
        );

        let property_snapshot = |name: &str| {
            project
                .read()
                .ok()
                .and_then(|project| node_property(&project, node_id, name))
        };
        {
            let mut context = ActionContext::new(
                &mut service,
                &mut history,
                PropertyOwner::Node(node_id),
                1.25,
            );
            assert!(context.handle_actions(
                vec![PropertyAction::SetMode(
                    "value".to_string(),
                    PropertyAuthoringMode::Keyframe,
                    initial_value,
                )],
                property_snapshot,
            ));
        }
        let before_picker = project
            .read()
            .map_err(|_| io::Error::other("Project read lock poisoned"))?
            .clone();
        let before_depth = history.undo_depth();
        let edited = PropertyValue::ColorValue(library::model::property::ColorValue::new(
            library::model::property::ColorSpaceRef::linear_srgb(),
            [0.25, 0.5, 0.75, 0.625],
        )?);
        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Node(node_id),
            1.25,
        );
        assert!(context.handle_actions(
            vec![
                PropertyAction::Update("value".to_string(), edited.clone()),
                PropertyAction::Commit,
            ],
            property_snapshot,
        ));
        assert_eq!(history.undo_depth(), before_depth + 1);
        let current = project
            .read()
            .map_err(|_| io::Error::other("Project read lock poisoned"))?
            .clone();
        let property = node_property(&current, node_id, "value")
            .ok_or_else(|| io::Error::other("Color property disappeared"))?;
        assert_eq!(property.evaluator, "keyframe");
        assert_eq!(
            property
                .keyframes()
                .into_iter()
                .find(|keyframe| keyframe.time == OrderedFloat(1.25))
                .map(|keyframe| keyframe.value),
            Some(edited)
        );
        assert_eq!(history.undo(&current), Some(before_picker));
        Ok(())
    }

    #[test]
    fn inspector_rejects_an_undeclared_property_without_mutation_or_history() {
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
        let before = project.read().unwrap().clone();
        let initial_depth = history.undo_depth();

        {
            let mut context = ActionContext::new(
                &mut service,
                &mut history,
                PropertyOwner::Node(node_id),
                1.25,
            );

            assert!(!context.handle_toggle_keyframe("new_amount", number(7.5), |_| None,));
        }

        assert_eq!(*project.read().unwrap(), before);
        assert_eq!(history.undo_depth(), initial_depth);
        assert!(node_property(&project.read().unwrap(), node_id, "new_amount").is_none());
    }

    #[test]
    fn expression_mode_roundtrip_preserves_its_authored_typed_fallback() -> TestResult {
        let expression = Property::expression("value * 2".to_string(), number(3.0));
        let constant = property_for_mode(
            Some(&expression),
            PropertyAuthoringMode::Constant,
            number(999.0),
            4.0,
        )
        .map_err(io::Error::other)?;
        assert_eq!(constant.evaluator, "constant");
        assert_eq!(constant.value(), Some(&number(3.0)));

        let keyframed = property_for_mode(
            Some(&constant),
            PropertyAuthoringMode::Keyframe,
            number(8.0),
            4.0,
        )
        .map_err(io::Error::other)?;
        assert_eq!(keyframed.evaluator, "keyframe");
        let keyframes = keyframed.keyframes();
        let keyframe = keyframes
            .first()
            .ok_or_else(|| io::Error::other("Keyframe mode created no key"))?;
        assert_eq!(keyframe.time, OrderedFloat(4.0));
        assert_eq!(keyframe.value, number(8.0));

        let malformed = Property {
            evaluator: "expression".to_string(),
            properties: std::collections::HashMap::from([(
                "expression".to_string(),
                PropertyValue::String("1".to_string()),
            )]),
        };
        assert!(property_for_mode(
            Some(&malformed),
            PropertyAuthoringMode::Constant,
            number(1.0),
            0.0,
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn inspector_authors_expression_source_and_fallback_on_the_project() -> TestResult {
        let plugins = Arc::new(PluginManager::default());
        let node = plugins.create_effect_operation_node("blur")?;
        let node_id = node.id;
        let mut initial = Project::new("expression authoring");
        initial.add_node(node);
        let project = Arc::new(RwLock::new(initial));
        let mut service =
            EditorService::new(Arc::clone(&project), plugins, Arc::new(CacheManager::new()))?;
        let mut history = HistoryManager::new();
        history.push_project_state(
            project
                .read()
                .map_err(|_| io::Error::other("Project read lock poisoned"))?
                .clone(),
        );

        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Node(node_id),
            1.5,
        );
        let property_snapshot = |name: &str| {
            project
                .read()
                .ok()
                .and_then(|project| node_property(&project, node_id, name))
        };
        assert!(context.handle_actions(
            vec![PropertyAction::SetMode(
                "sigma_x".to_string(),
                PropertyAuthoringMode::Expression,
                number(12.0),
            )],
            property_snapshot,
        ));
        assert!(context.handle_actions(
            vec![PropertyAction::SetExpressionSource(
                "sigma_x".to_string(),
                "value + sin(time)".to_string(),
            )],
            property_snapshot,
        ));
        assert!(context.handle_actions(
            vec![PropertyAction::Update("sigma_x".to_string(), number(4.0),)],
            property_snapshot,
        ));
        context.handle_commit();

        let expression = project
            .read()
            .ok()
            .and_then(|project| node_property(&project, node_id, "sigma_x"))
            .ok_or_else(|| io::Error::other("Expression property disappeared"))?;
        assert_eq!(expression.evaluator, "expression");
        assert_eq!(expression.expression_text(), Some("value + sin(time)"));
        assert_eq!(expression.value(), Some(&number(4.0)));

        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Node(node_id),
            1.5,
        );
        assert!(context.handle_actions(
            vec![PropertyAction::SetMode(
                "sigma_x".to_string(),
                PropertyAuthoringMode::Constant,
                number(999.0),
            )],
            |name| {
                project
                    .read()
                    .ok()
                    .and_then(|project| node_property(&project, node_id, name))
            },
        ));
        let constant = project
            .read()
            .ok()
            .and_then(|project| node_property(&project, node_id, "sigma_x"))
            .ok_or_else(|| io::Error::other("Constant property disappeared"))?;
        assert_eq!(constant.evaluator, "constant");
        assert_eq!(constant.value(), Some(&number(4.0)));
        Ok(())
    }

    #[test]
    fn clip_semantic_inspector_uses_the_same_expression_authoring_action() -> TestResult {
        let mut clip = Clip::new("semantic clip", 0.0, 5.0);
        clip.properties
            .set("amount".to_string(), Property::constant(number(2.0)));
        let clip_id = clip.id;
        let mut initial = Project::new("clip expression authoring");
        initial.add_clip(clip);
        let project = Arc::new(RwLock::new(initial));
        let mut service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )?;
        let mut history = HistoryManager::new();
        history.push_project_state(
            project
                .read()
                .map_err(|_| io::Error::other("Project read lock poisoned"))?
                .clone(),
        );

        let mut context = ActionContext::new(
            &mut service,
            &mut history,
            PropertyOwner::Clip(clip_id),
            0.5,
        );
        let property_snapshot = |name: &str| {
            project.read().ok().and_then(|project| {
                project
                    .get_clip(clip_id)
                    .and_then(|clip| clip.properties.get(name))
                    .cloned()
            })
        };
        assert!(context.handle_actions(
            vec![PropertyAction::SetMode(
                "amount".to_string(),
                PropertyAuthoringMode::Expression,
                number(2.0),
            )],
            property_snapshot,
        ));
        assert!(context.handle_actions(
            vec![PropertyAction::SetExpressionSource(
                "amount".to_string(),
                "value + time".to_string(),
            )],
            property_snapshot,
        ));
        assert!(context.handle_actions(
            vec![PropertyAction::Update("amount".to_string(), number(3.0),)],
            property_snapshot,
        ));

        let project = project
            .read()
            .map_err(|_| io::Error::other("Project read lock poisoned"))?;
        let property = project
            .get_clip(clip_id)
            .and_then(|clip| clip.properties.get("amount"))
            .ok_or_else(|| io::Error::other("Clip Expression property disappeared"))?;
        assert_eq!(property.evaluator, "expression");
        assert_eq!(property.expression_text(), Some("value + time"));
        assert_eq!(property.value(), Some(&number(3.0)));
        Ok(())
    }
}
