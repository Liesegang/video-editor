use crate::error::LibraryError;

use super::property_ops::{PropertyOwner, property_map, property_map_mut};
use crate::model::project::Project;
use crate::model::property::{KeyframeId, KeyframeUpdate, Property, PropertyValue};
use std::collections::{HashMap, hash_map::Entry};
use std::sync::{Arc, RwLock};

pub struct KeyframeHandler;

/// One keyframe mutation within an atomic batch.
///
/// The owner, property key, and persistent keyframe ID together
/// form the complete address. This deliberately carries no detached Project
/// state: [`KeyframeHandler`] resolves every address against the authoritative
/// model while holding one write lock.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyframeBatchUpdate {
    pub owner: PropertyOwner,
    pub property_key: String,
    pub keyframe_id: KeyframeId,
    pub update: KeyframeUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PropertyAddress {
    owner: PropertyOwner,
    property_key: String,
}

impl KeyframeHandler {
    /// Add a keyframe to an explicitly owned property.
    pub fn add_keyframe(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        Self::add_keyframe_with_id(project, owner, property_key, time, value, easing).map(|_| ())
    }

    pub fn add_keyframe_with_id(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<KeyframeId, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let prop_map = property_map_mut(&mut proj, owner)?;

        prop_map
            .upsert_keyframe_with_id(property_key, time, value, easing)
            .ok_or_else(|| {
                LibraryError::Project(format!("Property {} cannot be keyframed", property_key))
            })
    }

    /// Update a keyframe by persistent model identity. Prefer this for any
    /// continuous interaction because editing time can change sorted indices.
    pub fn update_keyframe_by_id(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        property_key: &str,
        keyframe_id: KeyframeId,
        update: KeyframeUpdate,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let prop_map = property_map_mut(&mut proj, owner)?;
        let property = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

        if !property.update_keyframe_by_id(keyframe_id, update) {
            return Err(LibraryError::Project(format!(
                "Failed to update keyframe {keyframe_id} for property {property_key}"
            )));
        }

        Ok(())
    }

    /// Atomically update keyframes across property owners.
    ///
    /// Only the affected [`Property`] values are cloned. Every update is
    /// validated and applied to those staged properties first; the
    /// authoritative Project is changed only after the complete batch has
    /// succeeded. The write lock is held for the whole transaction.
    pub fn update_keyframes_batch(
        project: &Arc<RwLock<Project>>,
        updates: &[KeyframeBatchUpdate],
    ) -> Result<(), LibraryError> {
        if updates.is_empty() {
            return Err(LibraryError::Project(
                "Keyframe update batch cannot be empty".to_string(),
            ));
        }

        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let mut staged = HashMap::<PropertyAddress, Property>::new();

        for update in updates {
            let address = PropertyAddress {
                owner: update.owner,
                property_key: update.property_key.clone(),
            };
            let property = match staged.entry(address) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let address = entry.key();
                    let property = property_map(&project, address.owner)?
                        .get(&address.property_key)
                        .cloned()
                        .ok_or_else(|| {
                            LibraryError::Project(format!(
                                "Property {} not found on {:?}",
                                address.property_key, address.owner
                            ))
                        })?;
                    entry.insert(property)
                }
            };
            if !property.update_keyframe_by_id(update.keyframe_id, update.update.clone()) {
                return Err(LibraryError::Project(format!(
                    "Failed to update keyframe {} for property {} on {:?}",
                    update.keyframe_id, update.property_key, update.owner
                )));
            }
        }

        // Revalidate every commit address before changing the Project. This
        // makes the following replacement pass infallible under the same lock:
        // replacing a Property cannot remove an owner or nested target.
        for address in staged.keys() {
            if property_map(&project, address.owner)?
                .get(&address.property_key)
                .is_none()
            {
                return Err(LibraryError::Project(format!(
                    "Property {} disappeared from {:?}",
                    address.property_key, address.owner
                )));
            }
        }
        for (address, property) in staged {
            let map = property_map_mut(&mut project, address.owner).map_err(|_| {
                LibraryError::Runtime(
                    "Validated property owner changed while its write lock was held".to_string(),
                )
            })?;
            map.set(address.property_key, property);
        }

        Ok(())
    }

    /// Remove a keyframe by persistent model identity.
    pub fn remove_keyframe_by_id(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let prop_map = property_map_mut(&mut proj, owner)?;
        let property = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

        if !property.remove_keyframe_by_id(keyframe_id) {
            return Err(LibraryError::Project(format!(
                "Failed to remove keyframe {keyframe_id} for property {property_key}"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::EasingFunction;
    use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
    use crate::model::frame::color::Color;
    use crate::model::property::{Keyframe, PropertyMap, PropertyValue};
    use crate::model::{Node, PluginOperationContent};
    use ordered_float::OrderedFloat;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    fn keyframed(value: f64) -> (Property, KeyframeId) {
        let keyframe = Keyframe::new(1.0, number(value), EasingFunction::Linear);
        let id = keyframe.id;
        (Property::keyframe(vec![keyframe]), id)
    }

    #[test]
    fn handler_uses_stable_identity_across_sorted_index_changes() {
        let mut project = Project::new("keyframes");
        let node = test_generator_node(
            "solid",
            GeneratorNodeRequest::Solid {
                color: Color::white(),
            },
        );
        let node_id = node.id;
        project.add_node(node);
        let project = Arc::new(RwLock::new(project));
        let owner = PropertyOwner::Node(node_id);

        let moving_id = KeyframeHandler::add_keyframe_with_id(
            &project,
            owner,
            "opacity",
            1.0,
            number(10.0),
            None,
        )
        .expect("missing property should be promoted");
        let stationary_id = KeyframeHandler::add_keyframe_with_id(
            &project,
            owner,
            "opacity",
            2.0,
            number(20.0),
            None,
        )
        .expect("second key should be inserted");

        KeyframeHandler::update_keyframe_by_id(
            &project,
            owner,
            "opacity",
            moving_id,
            KeyframeUpdate {
                time: Some(3.0),
                value: Some(number(30.0)),
                ..Default::default()
            },
        )
        .expect("moving key should cross the other key");
        KeyframeHandler::update_keyframe_by_id(
            &project,
            owner,
            "opacity",
            moving_id,
            KeyframeUpdate {
                value: Some(number(40.0)),
                ..Default::default()
            },
        )
        .expect("subsequent update should still target the moving key");

        let read = project.read().expect("project should remain readable");
        let property = read
            .get_node(node_id)
            .and_then(|node| node.properties.get("opacity"))
            .expect("keyframed property should exist");
        assert_eq!(
            property
                .keyframe_by_id(moving_id)
                .expect("moving key should exist")
                .value,
            number(40.0)
        );
        assert_eq!(
            property
                .keyframe_by_id(stationary_id)
                .expect("stationary key should exist")
                .value,
            number(20.0)
        );
    }

    #[test]
    fn batch_updates_direct_properties_on_operation_nodes_atomically() {
        let mut model = Project::new("atomic keyframe batch");
        let mut addresses = Vec::new();
        for (category, initial, updated) in [
            ("effect", 10.0, 11.0),
            ("style", 20.0, 21.0),
            ("effector", 30.0, 31.0),
            ("decorator", 40.0, 41.0),
        ] {
            let (property, keyframe_id) = keyframed(initial);
            let mut properties = PropertyMap::new();
            properties.set("amount".to_string(), property);
            let node = Node::new_plugin_operation(
                category,
                PluginOperationContent {
                    category: category.to_string(),
                    component_id: "test".to_string(),
                    operation: "test.apply.v1".to_string(),
                    declared_ports: Vec::new(),
                },
                properties,
            );
            addresses.push((PropertyOwner::Node(node.id), keyframe_id, updated));
            model.add_node(node);
        }
        let project = Arc::new(RwLock::new(model));
        let updates = addresses
            .iter()
            .map(|(owner, keyframe_id, value)| KeyframeBatchUpdate {
                owner: *owner,
                property_key: "amount".to_string(),
                keyframe_id: *keyframe_id,
                update: KeyframeUpdate {
                    time: Some(2.0),
                    value: Some(number(*value)),
                    ..Default::default()
                },
            })
            .collect::<Vec<_>>();

        KeyframeHandler::update_keyframes_batch(&project, &updates)
            .expect("all operation Nodes should update in one batch");
        let read = project.read().unwrap();
        for (owner, keyframe_id, value) in &addresses {
            let keyframe = read
                .get_node(owner.id())
                .unwrap()
                .properties
                .get("amount")
                .unwrap()
                .keyframe_by_id(*keyframe_id)
                .unwrap();
            assert_eq!(keyframe.time.into_inner(), 2.0);
            assert_eq!(keyframe.value, number(*value));
        }
        drop(read);

        let before_rejected_batch = project.read().unwrap().clone();
        let (first_owner, first_keyframe_id, _) = addresses[0];
        let (second_owner, _, _) = addresses[1];
        let rejected = [
            KeyframeBatchUpdate {
                owner: first_owner,
                property_key: "amount".to_string(),
                keyframe_id: first_keyframe_id,
                update: KeyframeUpdate {
                    value: Some(number(999.0)),
                    ..Default::default()
                },
            },
            KeyframeBatchUpdate {
                owner: second_owner,
                property_key: "amount".to_string(),
                keyframe_id: KeyframeId::new(),
                update: KeyframeUpdate {
                    value: Some(number(999.0)),
                    ..Default::default()
                },
            },
        ];
        assert!(KeyframeHandler::update_keyframes_batch(&project, &rejected).is_err());
        assert_eq!(*project.read().unwrap(), before_rejected_batch);
    }
}
