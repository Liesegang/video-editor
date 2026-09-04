use super::*;

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

    pub fn set_text(
        &self,
        item_id: TimelineItemId,
        text: String,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            let SourceRef::Text { text: authored } = &mut item.source else {
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
                if properties.get(&key).is_none() {
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

    pub fn update_authored_property_keyframe(
        &self,
        owner: AuthoringPropertyOwner,
        key: &str,
        keyframe_id: KeyframeId,
        update: AuthoringKeyframeUpdate,
    ) -> Result<ChangeSet, LibraryError> {
        if update.time.is_some_and(MediaTime::is_negative) {
            return Err(LibraryError::Validation(
                "Keyframe time must be non-negative".to_string(),
            ));
        }
        let mut session = self.write_session()?;
        let invalidations = property_owner_invalidations(session.project(), owner)?;
        session
            .transact(invalidations, |project| {
                let property = authored_properties_mut(project, owner)?
                    .get_mut(key)
                    .ok_or_else(|| format!("Missing authored Property '{key}'"))?;
                let updated = property.update_keyframe_by_id(
                    keyframe_id,
                    crate::model::property::KeyframeUpdate {
                        time: update.time.map(MediaTime::to_seconds_f64),
                        value: update.value,
                        easing: update.easing,
                    },
                );
                updated
                    .then_some(())
                    .ok_or_else(|| format!("Missing Keyframe {keyframe_id}"))
            })
            .map(|(_, changes)| changes)
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

    fn edit_item(
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

fn authored_properties_mut(
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
    }
}

fn property_owner_invalidations(
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
    })
}
