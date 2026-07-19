use crate::error::LibraryError;

use super::property_ops::{PropertyOwner, property_map, property_map_mut};
use crate::model::project::Project;
use crate::model::property::{KeyframeId, KeyframeUpdate, Property, PropertyTarget, PropertyValue};
use std::collections::{HashMap, hash_map::Entry};
use std::sync::{Arc, RwLock};

pub struct KeyframeHandler;

/// One keyframe mutation within an atomic batch.
///
/// The owner, nested target, property key, and persistent keyframe ID together
/// form the complete address. This deliberately carries no detached Project
/// state: [`KeyframeHandler`] resolves every address against the authoritative
/// model while holding one write lock.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyframeBatchUpdate {
    pub owner: PropertyOwner,
    pub target: PropertyTarget,
    pub property_key: String,
    pub keyframe_id: KeyframeId,
    pub update: KeyframeUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PropertyAddress {
    owner: PropertyOwner,
    target: PropertyTarget,
    property_key: String,
}

impl KeyframeHandler {
    /// Add a keyframe to an explicitly owned direct/effect/style property.
    pub fn add_keyframe(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        target: PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        Self::add_keyframe_with_id(project, owner, target, property_key, time, value, easing)
            .map(|_| ())
    }

    pub fn add_keyframe_with_id(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        target: PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<KeyframeId, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let prop_map = property_map_mut(&mut proj, owner, target)?;

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
        target: PropertyTarget,
        property_key: &str,
        keyframe_id: KeyframeId,
        update: KeyframeUpdate,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let prop_map = property_map_mut(&mut proj, owner, target)?;
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

    /// Atomically update keyframes across direct and nested property targets.
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
                target: update.target,
                property_key: update.property_key.clone(),
            };
            let property = match staged.entry(address) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let address = entry.key();
                    let property = property_map(&project, address.owner, address.target)?
                        .get(&address.property_key)
                        .cloned()
                        .ok_or_else(|| {
                            LibraryError::Project(format!(
                                "Property {} not found on {:?} {:?}",
                                address.property_key, address.owner, address.target
                            ))
                        })?;
                    entry.insert(property)
                }
            };
            if !property.update_keyframe_by_id(update.keyframe_id, update.update.clone()) {
                return Err(LibraryError::Project(format!(
                    "Failed to update keyframe {} for property {} on {:?} {:?}",
                    update.keyframe_id, update.property_key, update.owner, update.target
                )));
            }
        }

        // Revalidate every commit address before changing the Project. This
        // makes the following replacement pass infallible under the same lock:
        // replacing a Property cannot remove an owner or nested target.
        for address in staged.keys() {
            if property_map(&project, address.owner, address.target)?
                .get(&address.property_key)
                .is_none()
            {
                return Err(LibraryError::Project(format!(
                    "Property {} disappeared from {:?} {:?}",
                    address.property_key, address.owner, address.target
                )));
            }
        }
        for (address, property) in staged {
            let map =
                property_map_mut(&mut project, address.owner, address.target).map_err(|_| {
                    LibraryError::Runtime(
                        "Validated property target changed while its write lock was held"
                            .to_string(),
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
        target: PropertyTarget,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let prop_map = property_map_mut(&mut proj, owner, target)?;
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
    use crate::model::ensemble::{DecoratorInstance, EffectorInstance};
    use crate::model::property::{Keyframe, PropertyMap, PropertyValue};
    use crate::model::style::StyleInstance;
    use crate::model::{EffectConfig, GeneratorContent, Node, NodeContent};
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
        let node = Node::new("solid", NodeContent::Generator(GeneratorContent::Solid));
        let node_id = node.id;
        project.add_node(node);
        let project = Arc::new(RwLock::new(project));
        let owner = PropertyOwner::Node(node_id);

        let moving_id = KeyframeHandler::add_keyframe_with_id(
            &project,
            owner,
            PropertyTarget::Direct,
            "opacity",
            1.0,
            number(10.0),
            None,
        )
        .expect("missing property should be promoted");
        let stationary_id = KeyframeHandler::add_keyframe_with_id(
            &project,
            owner,
            PropertyTarget::Direct,
            "opacity",
            2.0,
            number(20.0),
            None,
        )
        .expect("second key should be inserted");

        KeyframeHandler::update_keyframe_by_id(
            &project,
            owner,
            PropertyTarget::Direct,
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
            PropertyTarget::Direct,
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
    fn batch_updates_every_target_and_rejects_partial_mutation() {
        let effect_id = uuid::Uuid::new_v4();
        let style_id = uuid::Uuid::new_v4();
        let effector_id = uuid::Uuid::new_v4();
        let decorator_id = uuid::Uuid::new_v4();
        let (direct, direct_keyframe_id) = keyframed(10.0);
        let (effect, effect_keyframe_id) = keyframed(20.0);
        let (style, style_keyframe_id) = keyframed(30.0);
        let (effector, effector_keyframe_id) = keyframed(40.0);
        let (decorator, decorator_keyframe_id) = keyframed(50.0);

        let mut node = Node::new("batch", NodeContent::Generator(GeneratorContent::Solid));
        let node_id = node.id;
        node.properties.set("amount".to_string(), direct);

        let mut properties = PropertyMap::new();
        properties.set("amount".to_string(), effect);
        node.effects.push(EffectConfig {
            id: effect_id,
            effect_type: "test".to_string(),
            properties,
        });

        let mut properties = PropertyMap::new();
        properties.set("amount".to_string(), style);
        let mut style = StyleInstance::new("test", properties);
        style.id = style_id;
        node.styles.push(style);

        let mut properties = PropertyMap::new();
        properties.set("amount".to_string(), effector);
        let mut effector = EffectorInstance::new("test", properties);
        effector.id = effector_id;
        node.effectors.push(effector);

        let mut properties = PropertyMap::new();
        properties.set("amount".to_string(), decorator);
        let mut decorator = DecoratorInstance::new("test", properties);
        decorator.id = decorator_id;
        node.decorators.push(decorator);

        let mut model = Project::new("atomic keyframe batch");
        model.add_node(node);
        let project = Arc::new(RwLock::new(model));
        let owner = PropertyOwner::Node(node_id);
        let addresses = [
            (PropertyTarget::Direct, direct_keyframe_id, 11.0),
            (PropertyTarget::Effect(effect_id), effect_keyframe_id, 21.0),
            (PropertyTarget::Style(style_id), style_keyframe_id, 31.0),
            (
                PropertyTarget::Effector(effector_id),
                effector_keyframe_id,
                41.0,
            ),
            (
                PropertyTarget::Decorator(decorator_id),
                decorator_keyframe_id,
                51.0,
            ),
        ];
        let updates = addresses
            .iter()
            .map(|(target, keyframe_id, value)| KeyframeBatchUpdate {
                owner,
                target: *target,
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
            .expect("all target types should update in one batch");
        let read = project.read().unwrap();
        let node = read.get_node(node_id).unwrap();
        for (target, keyframe_id, value) in addresses {
            let keyframe = node
                .property_map(target)
                .unwrap()
                .get("amount")
                .unwrap()
                .keyframe_by_id(keyframe_id)
                .unwrap();
            assert_eq!(keyframe.time.into_inner(), 2.0);
            assert_eq!(keyframe.value, number(value));
        }
        drop(read);

        let before_rejected_batch = project.read().unwrap().clone();
        let rejected = [
            KeyframeBatchUpdate {
                owner,
                target: PropertyTarget::Direct,
                property_key: "amount".to_string(),
                keyframe_id: direct_keyframe_id,
                update: KeyframeUpdate {
                    value: Some(number(999.0)),
                    ..Default::default()
                },
            },
            KeyframeBatchUpdate {
                owner,
                target: PropertyTarget::Effect(effect_id),
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
