use super::*;
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct TimelineSettingsUpdate {
    pub name: Option<String>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub fps: Option<RationalRate>,
    pub duration: Option<MediaTime>,
    pub background_color: Option<Color>,
    pub color_profile: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum AuthoringPropertyOwner {
    Timeline(TimelineId),
    Track(TimelineTrackId),
    Item(TimelineItemId),
    TextEnsemble {
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
    },
    Appearance {
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
    },
}

/// The existing authored evaluator that one direct-manipulation value edit
/// must preserve. Changing evaluator ownership is a separate explicit command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthoringPropertyValueTarget {
    Constant,
    Keyframe { local_time: MediaTime },
}

/// One value in an atomic multi-property direct-manipulation edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoringPropertyValueUpdate {
    pub key: String,
    pub value: PropertyValue,
    pub target: AuthoringPropertyValueTarget,
}

#[derive(Clone, Debug, Default)]
pub struct AuthoringKeyframeUpdate {
    pub time: Option<MediaTime>,
    pub value: Option<PropertyValue>,
    pub easing: Option<EasingFunction>,
}

impl TimelineEditorService {
    pub fn update_timeline_settings(
        &self,
        timeline_id: TimelineId,
        update: TimelineSettingsUpdate,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let timeline = project
                        .timelines
                        .get_mut(&timeline_id)
                        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
                    if let Some(name) = update.name {
                        timeline.name = name;
                    }
                    if let Some(width) = update.width {
                        timeline.width = width;
                    }
                    if let Some(height) = update.height {
                        timeline.height = height;
                    }
                    if let Some(fps) = update.fps {
                        timeline.fps = fps;
                    }
                    if let Some(duration) = update.duration {
                        timeline.duration = duration;
                    }
                    if let Some(background_color) = update.background_color {
                        timeline.background_color = background_color;
                    }
                    if let Some(color_profile) = update.color_profile {
                        timeline.color_profile = color_profile;
                    }
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn rename_track(
        &self,
        track_id: TimelineTrackId,
        name: String,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_track(session.project(), track_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    project
                        .tracks
                        .get_mut(&track_id)
                        .ok_or_else(|| format!("Missing Timeline Track {track_id}"))?
                        .name = name;
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn reorder_track(
        &self,
        timeline_id: TimelineId,
        track_id: TimelineTrackId,
        new_index: usize,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let timeline = project
                        .timelines
                        .get_mut(&timeline_id)
                        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
                    let old_index = timeline
                        .track_order
                        .iter()
                        .position(|candidate| *candidate == track_id)
                        .ok_or_else(|| {
                            format!("Timeline {timeline_id} does not own Track {track_id}")
                        })?;
                    if new_index >= timeline.track_order.len() {
                        return Err(format!(
                            "Track index {new_index} is outside Timeline {timeline_id}"
                        ));
                    }
                    let moved = timeline.track_order.remove(old_index);
                    timeline.track_order.insert(new_index, moved);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Enables or disables only a Track's visual contribution in one
    /// undoable authored transaction. Audio and AudioVisual sound output are
    /// intentionally not muted by this control.
    pub fn set_track_visual_enabled(
        &self,
        track_id: TimelineTrackId,
        enabled: bool,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_track(session.project(), track_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let properties = &mut project
                        .tracks
                        .get_mut(&track_id)
                        .ok_or_else(|| format!("Missing Timeline Track {track_id}"))?
                        .authored_properties;
                    if enabled {
                        properties.remove(TRACK_VISIBILITY_PROPERTY);
                    } else {
                        properties.set(
                            TRACK_VISIBILITY_PROPERTY.to_string(),
                            Property::constant(PropertyValue::Boolean(false)),
                        );
                    }
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn rename_item(
        &self,
        item_id: TimelineItemId,
        name: String,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            item.name = name;
            Ok(())
        })
    }

    pub fn set_item_parent(
        &self,
        item_id: TimelineItemId,
        parent: Option<TimelineItemId>,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            item.parent = parent;
            Ok(())
        })
    }

    /// Changes only the selected Timeline placement's compositing mode.
    /// Reusable Module definitions and sibling instances remain untouched.
    pub fn set_item_blend_mode(
        &self,
        item_id: TimelineItemId,
        blend_mode: BlendMode,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            item.blend_mode = blend_mode;
            Ok(())
        })
    }

    pub fn set_text(
        &self,
        item_id: TimelineItemId,
        text: String,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            let SourceRef::Text { text: authored, .. } = &mut item.source else {
                return Err(format!("Timeline item {item_id} is not Text"));
            };
            *authored = text;
            Ok(())
        })
    }

    pub fn set_authored_property(
        &self,
        owner: AuthoringPropertyOwner,
        key: String,
        property: Property,
    ) -> Result<ChangeSet, LibraryError> {
        if property.evaluator == "keyframe" {
            return Err(LibraryError::Validation(
                "Use exact-time authored Keyframe commands instead of replacing a Keyframe Property"
                    .to_string(),
            ));
        }
        let mut session = self.write_session()?;
        let invalidations = property_owner_invalidations(session.project(), owner)?;
        session
            .transact(invalidations, |project| {
                authored_properties_mut(project, owner)?.set(key, property);
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn set_authored_property_constant(
        &self,
        owner: AuthoringPropertyOwner,
        key: String,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        self.set_authored_property(owner, key, Property::constant(value))
    }

    /// Applies several sampled authored values in one Project transaction.
    ///
    /// Preview resize gestures can change both Scale and Position. Treating
    /// those as separate commands would expose an invalid intermediate frame
    /// and split one pointer gesture across two undo entries. The caller must
    /// state the evaluator observed when the gesture began; this method
    /// refuses stale or expression-controlled ownership instead of replacing
    /// it silently.
    pub fn apply_authored_property_values(
        &self,
        owner: AuthoringPropertyOwner,
        updates: Vec<AuthoringPropertyValueUpdate>,
    ) -> Result<ChangeSet, LibraryError> {
        validate_authored_property_updates(&updates).map_err(LibraryError::Validation)?;

        let mut session = self.write_session()?;
        let invalidations = property_owner_invalidations(session.project(), owner)?;
        session
            .transact(invalidations, move |project| {
                let properties = authored_properties_mut(project, owner)?;
                apply_authored_property_updates(properties, updates)
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Builds the exact value projection used by direct manipulation without
    /// opening a transaction. Preview uses this immutable clone for live
    /// rendering; release applies the same policy through
    /// [`Self::apply_authored_property_values`].
    pub fn project_authored_property_values(
        project: &AuthoringProject,
        owner: AuthoringPropertyOwner,
        updates: Vec<AuthoringPropertyValueUpdate>,
    ) -> Result<AuthoringProject, LibraryError> {
        validate_authored_property_updates(&updates).map_err(LibraryError::Validation)?;
        let mut projected = project.clone();
        let properties =
            authored_properties_mut(&mut projected, owner).map_err(LibraryError::Validation)?;
        apply_authored_property_updates(properties, updates).map_err(LibraryError::Validation)?;
        Ok(projected)
    }

    pub fn remove_authored_property(
        &self,
        owner: AuthoringPropertyOwner,
        key: &str,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let invalidations = property_owner_invalidations(session.project(), owner)?;
        session
            .transact(invalidations, |project| {
                authored_properties_mut(project, owner)?
                    .remove(key)
                    .map(|_| ())
                    .ok_or_else(|| format!("Missing authored Property '{key}'"))
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn upsert_authored_property_keyframe(
        &self,
        owner: AuthoringPropertyOwner,
        key: String,
        local_time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<(KeyframeId, ChangeSet), LibraryError> {
        self.upsert_authored_property_keyframe_impl(owner, key, local_time, value, easing, false)
    }

    /// Explicitly switches any authored evaluator to Keyframe mode and places
    /// its first key at the requested local time. Unlike a normal upsert, this
    /// command intentionally replaces an Expression evaluator in one undoable
    /// model edit.
    pub fn set_authored_property_keyframe_mode(
        &self,
        owner: AuthoringPropertyOwner,
        key: String,
        local_time: MediaTime,
        value: PropertyValue,
    ) -> Result<(KeyframeId, ChangeSet), LibraryError> {
        self.upsert_authored_property_keyframe_impl(owner, key, local_time, value, None, true)
    }

    fn upsert_authored_property_keyframe_impl(
        &self,
        owner: AuthoringPropertyOwner,
        key: String,
        local_time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
        replace_evaluator: bool,
    ) -> Result<(KeyframeId, ChangeSet), LibraryError> {
        if local_time.is_negative() {
            return Err(LibraryError::Validation(
                "Keyframe time must be non-negative".to_string(),
            ));
        }
        let mut session = self.write_session()?;
        let invalidations = property_owner_invalidations(session.project(), owner)?;
        session
            .transact(invalidations, |project| {
                let properties = authored_properties_mut(project, owner)?;
                if properties.get(&key).is_none()
                    || (replace_evaluator
                        && properties
                            .get(&key)
                            .is_some_and(|property| property.evaluator != "keyframe"))
                {
                    properties.set(key.clone(), Property::constant(value.clone()));
                }
                properties
                    .get_mut(&key)
                    .and_then(|property| {
                        property.upsert_keyframe_with_id(local_time.to_seconds_f64(), value, easing)
                    })
                    .ok_or_else(|| format!("Property '{key}' does not support Keyframes"))
            })
            .map_err(LibraryError::Validation)
    }

    pub fn remove_authored_property_keyframe(
        &self,
        owner: AuthoringPropertyOwner,
        key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let invalidations = property_owner_invalidations(session.project(), owner)?;
        session
            .transact(invalidations, |project| {
                let property = authored_properties_mut(project, owner)?
                    .get_mut(key)
                    .ok_or_else(|| format!("Missing authored Property '{key}'"))?;
                property
                    .remove_keyframe_by_id(keyframe_id)
                    .then_some(())
                    .ok_or_else(|| format!("Missing Keyframe {keyframe_id}"))
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub(super) fn edit_item(
        &self,
        item_id: TimelineItemId,
        edit: impl FnOnce(&mut TimelineItem) -> Result<(), String>,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::Item {
                    timeline_id,
                    item_id,
                }],
                |project| {
                    edit(
                        project
                            .items
                            .get_mut(&item_id)
                            .ok_or_else(|| format!("Missing Timeline item {item_id}"))?,
                    )
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

fn validate_authored_property_updates(
    updates: &[AuthoringPropertyValueUpdate],
) -> Result<(), String> {
    if updates.is_empty() {
        return Err("An authored property value edit must contain at least one update".to_string());
    }
    let mut keys = HashSet::with_capacity(updates.len());
    for update in updates {
        if update.key.is_empty() {
            return Err("Authored Property key must not be empty".to_string());
        }
        if !keys.insert(update.key.clone()) {
            return Err(format!(
                "Authored Property '{}' appears more than once in one edit",
                update.key
            ));
        }
        if matches!(
            update.target,
            AuthoringPropertyValueTarget::Keyframe { local_time } if local_time.is_negative()
        ) {
            return Err("Keyframe time must be non-negative".to_string());
        }
    }
    Ok(())
}

fn apply_authored_property_updates(
    properties: &mut PropertyMap,
    updates: Vec<AuthoringPropertyValueUpdate>,
) -> Result<(), String> {
    for update in updates {
        match update.target {
            AuthoringPropertyValueTarget::Constant => {
                if let Some(property) = properties.get_mut(&update.key) {
                    if property.evaluator != "constant" {
                        return Err(format!(
                            "Authored Property '{}' changed from Constant to '{}' during direct manipulation",
                            update.key, property.evaluator
                        ));
                    }
                    *property = Property::constant(update.value);
                } else {
                    properties.set(update.key, Property::constant(update.value));
                }
            }
            AuthoringPropertyValueTarget::Keyframe { local_time } => {
                let property = properties.get_mut(&update.key).ok_or_else(|| {
                    format!(
                        "Authored Property '{}' disappeared during direct manipulation",
                        update.key
                    )
                })?;
                if property.evaluator != "keyframe" {
                    return Err(format!(
                        "Authored Property '{}' changed from Keyframe to '{}' during direct manipulation",
                        update.key, property.evaluator
                    ));
                }
                property
                    .upsert_keyframe_with_id(local_time.to_seconds_f64(), update.value, None)
                    .ok_or_else(|| {
                        format!(
                            "Authored Property '{}' no longer supports Keyframes",
                            update.key
                        )
                    })?;
            }
        }
    }
    Ok(())
}

pub(super) fn authored_properties_mut(
    project: &mut AuthoringProject,
    owner: AuthoringPropertyOwner,
) -> Result<&mut PropertyMap, String> {
    match owner {
        AuthoringPropertyOwner::Timeline(timeline_id) => project
            .timelines
            .get_mut(&timeline_id)
            .map(|timeline| &mut timeline.authored_properties)
            .ok_or_else(|| format!("Missing Timeline {timeline_id}")),
        AuthoringPropertyOwner::Track(track_id) => project
            .tracks
            .get_mut(&track_id)
            .map(|track| &mut track.authored_properties)
            .ok_or_else(|| format!("Missing Timeline Track {track_id}")),
        AuthoringPropertyOwner::Item(item_id) => project
            .items
            .get_mut(&item_id)
            .map(|item| &mut item.authored_properties)
            .ok_or_else(|| format!("Missing Timeline item {item_id}")),
        AuthoringPropertyOwner::TextEnsemble {
            item_id,
            operation_id,
        } => {
            let item = project
                .items
                .get_mut(&item_id)
                .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
            let SourceRef::Text {
                ensemble_operations,
                ..
            } = &mut item.source
            else {
                return Err(format!("Timeline item {item_id} is not Text"));
            };
            ensemble_operations
                .iter_mut()
                .find(|operation| operation.id == operation_id)
                .map(|operation| &mut operation.properties)
                .ok_or_else(|| format!("Missing Text Ensemble operation {operation_id}"))
        }
        AuthoringPropertyOwner::Appearance {
            item_id,
            operation_id,
        } => {
            let item = project
                .items
                .get_mut(&item_id)
                .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
            super::appearance::appearance_operations_mut(item, item_id)?
                .iter_mut()
                .find(|operation| operation.id == operation_id)
                .map(|operation| &mut operation.properties)
                .ok_or_else(|| format!("Missing Appearance operation {operation_id}"))
        }
    }
}

pub(super) fn property_owner_invalidations(
    project: &AuthoringProject,
    owner: AuthoringPropertyOwner,
) -> Result<Vec<ProjectInvalidation>, LibraryError> {
    Ok(match owner {
        AuthoringPropertyOwner::Timeline(timeline_id) => {
            if !project.timelines.contains_key(&timeline_id) {
                return Err(LibraryError::Validation(format!(
                    "Missing Timeline {timeline_id}"
                )));
            }
            vec![ProjectInvalidation::TimelineStructure { timeline_id }]
        }
        AuthoringPropertyOwner::Track(track_id) => vec![ProjectInvalidation::TimelineStructure {
            timeline_id: timeline_for_track(project, track_id)?,
        }],
        AuthoringPropertyOwner::Item(item_id) => vec![ProjectInvalidation::Item {
            timeline_id: timeline_for_item(project, item_id)?,
            item_id,
        }],
        AuthoringPropertyOwner::TextEnsemble { item_id, .. } => {
            vec![ProjectInvalidation::Item {
                timeline_id: timeline_for_item(project, item_id)?,
                item_id,
            }]
        }
        AuthoringPropertyOwner::Appearance { item_id, .. } => vec![ProjectInvalidation::Item {
            timeline_id: timeline_for_item(project, item_id)?,
            item_id,
        }],
    })
}
