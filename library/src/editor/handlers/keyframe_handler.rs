use crate::error::LibraryError;

use super::property_ops::{self, PropertyOwner, property_map};
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

        property_ops::upsert_keyframe_with_id(&mut proj, owner, property_key, time, value, easing)
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

        property_ops::update_keyframe_by_id(&mut proj, owner, property_key, keyframe_id, update)
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
            property_ops::replace_property(
                &mut project,
                address.owner,
                &address.property_key,
                property,
            )
            .map_err(|_| {
                LibraryError::Runtime(
                    "Validated property owner changed while its write lock was held".to_string(),
                )
            })?;
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

        property_ops::remove_keyframe_by_id(&mut proj, owner, property_key, keyframe_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::EasingFunction;
    use crate::model::property::{Keyframe, PropertyValue};
    use crate::plugin::PluginManager;
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
        let node = PluginManager::default()
            .create_style_operation_node("fill")
            .expect("Fill Style should be registered");
        let node_id = node.id;
        project.add_node(node);
        let project = Arc::new(RwLock::new(project));
        let owner = PropertyOwner::Node(node_id);

        let moving_id = KeyframeHandler::add_keyframe_with_id(
            &project,
            owner,
            "opacity",
            1.0,
            number(0.1),
            None,
        )
        .expect("initialized constant property should be promoted");
        let stationary_id = KeyframeHandler::add_keyframe_with_id(
            &project,
            owner,
            "opacity",
            2.0,
            number(0.2),
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
                value: Some(number(0.3)),
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
                value: Some(number(0.4)),
                ..Default::default()
            },
        )
        .expect("subsequent update should still target the moving key");

        let read = project.read().expect("project should remain readable");
        let property = read
            .get_node(node_id)
            .and_then(|node| node.properties().get("opacity"))
            .expect("keyframed property should exist");
        assert_eq!(
            property
                .keyframe_by_id(moving_id)
                .expect("moving key should exist")
                .value,
            number(0.4)
        );
        assert_eq!(
            property
                .keyframe_by_id(stationary_id)
                .expect("stationary key should exist")
                .value,
            number(0.2)
        );
    }

    #[test]
    fn batch_updates_direct_properties_on_operation_nodes_atomically() {
        let mut model = Project::new("atomic keyframe batch");
        let mut addresses = Vec::new();
        let plugins = PluginManager::default();
        let cases = [
            (
                plugins.create_effect_operation_node("blur"),
                "sigma_x",
                10.0,
                11.0,
            ),
            (
                plugins.create_style_operation_node("fill"),
                "opacity",
                20.0,
                21.0,
            ),
            (
                plugins.create_effector_operation_node("opacity"),
                "opacity",
                30.0,
                31.0,
            ),
            (
                plugins.create_decorator_operation_node("backplate"),
                "padding",
                40.0,
                41.0,
            ),
        ];
        for (node, property_key, initial, updated) in cases {
            let (property, keyframe_id) = keyframed(initial);
            let mut node = node.expect("registered operation creates a complete Node");
            node.set_property(property_key.to_string(), property)
                .expect("registered descriptor initializes its property");
            addresses.push((
                PropertyOwner::Node(node.id),
                property_key,
                keyframe_id,
                updated,
            ));
            model.add_node(node);
        }
        let project = Arc::new(RwLock::new(model));
        let updates = addresses
            .iter()
            .map(
                |(owner, property_key, keyframe_id, value)| KeyframeBatchUpdate {
                    owner: *owner,
                    property_key: (*property_key).to_string(),
                    keyframe_id: *keyframe_id,
                    update: KeyframeUpdate {
                        time: Some(2.0),
                        value: Some(number(*value)),
                        ..Default::default()
                    },
                },
            )
            .collect::<Vec<_>>();

        KeyframeHandler::update_keyframes_batch(&project, &updates)
            .expect("all operation Nodes should update in one batch");
        let read = project.read().unwrap();
        for (owner, property_key, keyframe_id, value) in &addresses {
            let keyframe = read
                .get_node(owner.id())
                .unwrap()
                .properties()
                .get(property_key)
                .unwrap()
                .keyframe_by_id(*keyframe_id)
                .unwrap();
            assert_eq!(keyframe.time.into_inner(), 2.0);
            assert_eq!(keyframe.value, number(*value));
        }
        drop(read);

        let before_rejected_batch = project.read().unwrap().clone();
        let (first_owner, first_property_key, first_keyframe_id, _) = addresses[0];
        let (second_owner, second_property_key, _, _) = addresses[1];
        let rejected = [
            KeyframeBatchUpdate {
                owner: first_owner,
                property_key: first_property_key.to_string(),
                keyframe_id: first_keyframe_id,
                update: KeyframeUpdate {
                    value: Some(number(999.0)),
                    ..Default::default()
                },
            },
            KeyframeBatchUpdate {
                owner: second_owner,
                property_key: second_property_key.to_string(),
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
