//! Runtime-aligned property clock for each Module document host.

use library::model::authoring::{AttachmentOwner, AuthoringProject, MediaTime, TimelineId};

use super::{ModuleEditorHost, ModulePropertyContext};
use crate::state::authoring::AutomationOwner;
use crate::ui::automation_lanes::{
    local_time_for_timeline, transition_owner as transition_automation_owner,
};

/// Build the one property context used by evaluation, keyframe mode controls,
/// toggles, and authored value edits in a Module document.
///
/// Node Clips and item Effects run in item-local source time, Track/Timeline
/// Effects run in Timeline time, and Transitions run from zero through their
/// derived interval duration. Keeping this conversion at the host boundary
/// prevents the Node Editor from authoring absolute-Timeline keyframes into a
/// processor whose runtime receives a local clock.
pub(super) fn module_property_context(
    project: &AuthoringProject,
    active_timeline_id: TimelineId,
    current_frame: i64,
    host: &ModuleEditorHost,
) -> ModulePropertyContext {
    let timeline = project
        .timelines
        .get(&active_timeline_id)
        .or_else(|| project.timelines.get(&project.root_timeline_id));
    let resolution = timeline
        .map(|timeline| (timeline.width, timeline.height))
        .unwrap_or((1920, 1080));
    let fps = timeline.map_or(30.0, |timeline| timeline.fps.to_f64());
    let timeline_time = timeline
        .and_then(|timeline| MediaTime::from_frame_index(current_frame, timeline.fps).ok())
        .unwrap_or_else(MediaTime::zero);
    let property_time = module_property_time(project, host, timeline_time).unwrap_or(timeline_time);
    ModulePropertyContext {
        time: property_time.to_seconds_f64(),
        fps,
        resolution,
    }
}

fn module_property_time(
    project: &AuthoringProject,
    host: &ModuleEditorHost,
    timeline_time: MediaTime,
) -> Option<MediaTime> {
    let owner = match host {
        ModuleEditorHost::NodeClip {
            timeline_item_id, ..
        } => AutomationOwner::Item(*timeline_item_id),
        ModuleEditorHost::Transition {
            transition_id,
            instance_path,
            ..
        } => transition_automation_owner(*transition_id, instance_path.as_ref()),
        ModuleEditorHost::Attachment { attachment_id, .. } => {
            match &project.attachments.get(attachment_id)?.owner {
                AttachmentOwner::Item { item_id } => AutomationOwner::Item(*item_id),
                // Track and Timeline processors intentionally share the
                // Timeline clock used by their runtime evaluation stages.
                AttachmentOwner::Track { .. } | AttachmentOwner::Timeline { .. } => {
                    return Some(timeline_time);
                }
            }
        }
    };
    local_time_for_timeline(project, &owner, timeline_time)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use library::animation::EasingFunction;
    use library::editor::TimelineEditorService;
    use library::model::authoring::{
        Attachment, AttachmentId, AttachmentProcessor, AttachmentStage, AuthoringProject,
        InstancePath, ModuleInstanceId, ModuleInvocation, ModuleOutputId, RationalRate, SourceRef,
        TimelineInterval, TimelineItemId, Transition, TransitionAlignment, TransitionId,
        TransitionProcessor,
    };
    use library::model::frame::color::Color;
    use library::model::property::{Keyframe, Property, PropertyValue};

    use super::*;
    use crate::state::node_editor::ModuleEditorHost;

    #[test]
    fn transition_properties_author_and_evaluate_on_a_bounded_local_clock() {
        let fps = RationalRate::new(30, 1).expect("fixture frame rate");
        let duration = MediaTime::from_whole_seconds(12);
        let mut project =
            AuthoringProject::new("transition clock", 320, 180, fps, duration).unwrap();
        let transition_id = TransitionId::new();
        project.transitions.insert(
            transition_id,
            Transition {
                id: transition_id,
                timeline_id: project.root_timeline_id,
                from_item_id: TimelineItemId::new(),
                to_item_id: TimelineItemId::new(),
                edit_point: MediaTime::from_whole_seconds(5),
                duration: MediaTime::from_whole_seconds(4),
                alignment: TransitionAlignment::CenteredOnEdit,
                processor: TransitionProcessor::cross_dissolve(),
                parameters: HashMap::new(),
            },
        );
        let host = ModuleEditorHost::Transition {
            transition_id,
            instance_path: Some(
                InstancePath::root(project.root_timeline_id).nested(TimelineItemId::new()),
            ),
            module_instance_id: ModuleInstanceId::new(),
        };

        // Centered interval is [3s, 7s]. The editor and runtime both see a
        // transition-local [0s, 4s] clock, even outside the active interval.
        for (frame, expected) in [(60, 0.0), (90, 0.0), (120, 1.0), (210, 4.0), (240, 4.0)] {
            let context = module_property_context(&project, project.root_timeline_id, frame, &host);
            assert_eq!(context.time, expected);
        }

        let context = module_property_context(&project, project.root_timeline_id, 90, &host);
        let property = Property::keyframe(vec![
            Keyframe::new(0.0, PropertyValue::Integer(1), EasingFunction::Linear),
            Keyframe::new(1.0, PropertyValue::Integer(2), EasingFunction::Linear),
        ]);
        assert_eq!(
            property.evaluate_at(context.time).unwrap(),
            PropertyValue::Integer(1)
        );
        let authored = super::super::property::property_with_edited_value(
            &property,
            PropertyValue::Integer(9),
            context.time,
        );
        assert_eq!(
            authored.evaluate_at(0.0).unwrap(),
            PropertyValue::Integer(9)
        );
        assert!(!authored
            .keyframes()
            .iter()
            .any(|keyframe| keyframe.time.into_inner().is_sign_negative()));
    }

    #[test]
    fn node_clip_property_clock_remains_item_local_and_bounded() {
        let fps = RationalRate::new(30, 1).expect("fixture frame rate");
        let duration = MediaTime::from_whole_seconds(12);
        let project = AuthoringProject::new("node clip clock", 320, 180, fps, duration).unwrap();
        let service = TimelineEditorService::new(project).unwrap();
        let project = service.snapshot().unwrap();
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        let (item_id, _) = service
            .add_item(
                track_id,
                "Node host".to_string(),
                SourceRef::Solid {
                    color: Color::black(),
                },
                TimelineInterval::new(
                    MediaTime::from_whole_seconds(3),
                    MediaTime::from_whole_seconds(5),
                )
                .unwrap(),
                0,
            )
            .unwrap();
        let project = service.snapshot().unwrap();
        let host = ModuleEditorHost::NodeClip {
            timeline_item_id: item_id,
            instance_path: None,
            module_instance_id: ModuleInstanceId::new(),
        };

        for (frame, expected) in [(60, 0.0), (90, 0.0), (120, 1.0), (240, 5.0), (270, 5.0)] {
            let context = module_property_context(&project, timeline_id, frame, &host);
            assert_eq!(context.time, expected);
        }
    }

    #[test]
    fn effect_property_clock_matches_its_item_or_timeline_runtime_stage() {
        let fps = RationalRate::new(30, 1).expect("fixture frame rate");
        let duration = MediaTime::from_whole_seconds(12);
        let project = AuthoringProject::new("effect clock", 320, 180, fps, duration).unwrap();
        let service = TimelineEditorService::new(project).unwrap();
        let project = service.snapshot().unwrap();
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        let (item_id, _) = service
            .add_item(
                track_id,
                "Effect host".to_string(),
                SourceRef::Solid {
                    color: Color::black(),
                },
                TimelineInterval::new(
                    MediaTime::from_whole_seconds(3),
                    MediaTime::from_whole_seconds(5),
                )
                .unwrap(),
                0,
            )
            .unwrap();
        let mut project = service.snapshot().unwrap().as_ref().clone();
        let attachment_id = AttachmentId::new();
        let instance_id = ModuleInstanceId::new();
        project.attachments.insert(
            attachment_id,
            Attachment {
                id: attachment_id,
                owner: AttachmentOwner::Item { item_id },
                stage: AttachmentStage::ItemPostTransform,
                order: 0,
                enabled: true,
                bypassed: false,
                processor: AttachmentProcessor::Module(ModuleInvocation {
                    instance_id,
                    output_id: ModuleOutputId::new(),
                    input_bindings: HashMap::new(),
                    automation_tracks: HashMap::new(),
                }),
            },
        );
        let host = ModuleEditorHost::Attachment {
            attachment_id,
            instance_path: None,
            module_instance_id: instance_id,
        };

        assert_eq!(
            module_property_context(&project, timeline_id, 120, &host).time,
            1.0,
            "item Effect must use item-local time"
        );
        project.attachments.get_mut(&attachment_id).unwrap().owner =
            AttachmentOwner::Track { track_id };
        project.attachments.get_mut(&attachment_id).unwrap().stage =
            AttachmentStage::TrackPostComposite;
        assert_eq!(
            module_property_context(&project, timeline_id, 120, &host).time,
            4.0,
            "Track Effect must keep Timeline time"
        );
    }
}
