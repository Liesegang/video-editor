use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use ordered_float::OrderedFloat;
use uuid::Uuid;

use super::property_ops::{PropertyOwner, set_property_attribute as set_property_attribute_value};
use crate::error::LibraryError;
use crate::model::project::{NodeContainer, NodeGraphBundle, Project};
use crate::model::property::PropertyValue;
use crate::model::{Clip, CompositionInstanceContent, Node, NodeContent};

/// A detached Clip graph prepared by the factory methods on ProjectManager.
/// It is inserted into Project atomically by `add_clip_to_track`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipBundle {
    pub clip: Clip,
    pub graph: NodeGraphBundle,
}

impl ClipBundle {
    pub fn with_image_node(mut clip: Clip, node: Node) -> Self {
        clip.node_ids = vec![node.id];
        clip.output_node_id = Some(node.id);
        Self {
            clip,
            graph: NodeGraphBundle::with_output_node(node),
        }
    }

    pub fn with_audio_node(mut clip: Clip, node: Node) -> Self {
        clip.node_ids = vec![node.id];
        clip.audio_output_node_id = Some(node.id);
        Self {
            clip,
            graph: NodeGraphBundle::new(vec![node], Vec::new(), None),
        }
    }

    pub fn with_av_node(mut clip: Clip, node: Node) -> Self {
        clip.node_ids = vec![node.id];
        clip.output_node_id = Some(node.id);
        clip.audio_output_node_id = Some(node.id);
        Self {
            clip,
            graph: NodeGraphBundle::with_output_node(node),
        }
    }

    pub fn primary_node(&self) -> Option<&Node> {
        self.graph.output_node().or_else(|| {
            let audio_output = self.clip.audio_output_node_id?;
            self.graph.nodes.iter().find(|node| node.id == audio_output)
        })
    }

    pub fn primary_node_mut(&mut self) -> Option<&mut Node> {
        let primary_id = self
            .graph
            .output_node_id
            .or(self.clip.audio_output_node_id)?;
        self.graph
            .nodes
            .iter_mut()
            .find(|node| node.id == primary_id)
    }
}

pub struct ClipHandler;

impl ClipHandler {
    /// Atomically inserts a detached Clip graph and attaches it to a top-level
    /// Track. No partially inserted Clip or Node remains on failure.
    pub fn add_clip_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        mut bundle: ClipBundle,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let composition = project.get_composition(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition {composition_id} not found"))
        })?;
        if !composition.track_ids.contains(&track_id) {
            return Err(LibraryError::Project(format!(
                "Track {track_id} is not in Composition {composition_id}"
            )));
        }
        if project.get_track(track_id).is_none() {
            return Err(LibraryError::Project(format!("Track {track_id} not found")));
        }
        if project.get_clip(bundle.clip.id).is_some() {
            return Err(LibraryError::Project(format!(
                "Clip {} already exists",
                bundle.clip.id
            )));
        }

        let mut node_ids = HashSet::new();
        for node in &bundle.graph.nodes {
            if !node_ids.insert(node.id) || project.get_node(node.id).is_some() {
                return Err(LibraryError::Project(format!(
                    "Node {} already exists in the Clip bundle or Project",
                    node.id
                )));
            }
            if let NodeContent::CompositionInstance(CompositionInstanceContent {
                composition_id: target_composition_id,
            }) = node.content()
                && !Self::validate_recursion(&project, *target_composition_id, composition_id)
            {
                return Err(LibraryError::Project(
                    "Cannot add composition: composition instance cycle detected".to_string(),
                ));
            }
        }
        if bundle.graph.nodes.is_empty() {
            return Err(LibraryError::Project(
                "A factory Clip must contain an explicit image or audio output Node".to_string(),
            ));
        }

        let clip_id = bundle.clip.id;
        let image_output_node_id = match (bundle.clip.output_node_id, bundle.graph.output_node_id) {
            (Some(clip_output), Some(graph_output)) if clip_output != graph_output => {
                return Err(LibraryError::Project(format!(
                    "Clip and graph disagree on explicit image output: {clip_output} != {graph_output}"
                )));
            }
            (Some(output), _) | (_, Some(output)) if node_ids.contains(&output) => Some(output),
            (Some(output), _) | (_, Some(output)) => {
                return Err(LibraryError::Project(format!(
                    "Explicit image output Node {output} is not bundled"
                )));
            }
            (None, None) => None,
        };
        let audio_output_node_id = match bundle.clip.audio_output_node_id {
            Some(output) if node_ids.contains(&output) => Some(output),
            Some(output) => {
                return Err(LibraryError::Project(format!(
                    "Explicit audio output Node {output} is not bundled"
                )));
            }
            None => None,
        };
        if image_output_node_id.is_none() && audio_output_node_id.is_none() {
            return Err(LibraryError::Project(
                "A factory Clip requires an explicit image or audio output Node".to_string(),
            ));
        }
        bundle.clip.node_ids.clear();
        bundle.clip.output_node_id = None;
        bundle.clip.audio_output_node_id = None;
        bundle.graph.output_node_id = image_output_node_id;
        project.add_clip(bundle.clip);

        let result = (|| {
            project
                .insert_node_graph(NodeContainer::Clip(clip_id), bundle.graph)
                .map_err(|error| LibraryError::Project(error.to_string()))?;
            project
                .set_audio_output_node(NodeContainer::Clip(clip_id), audio_output_node_id)
                .map_err(|error| LibraryError::Project(error.to_string()))?;
            project
                .attach_clip_to_track_at(track_id, clip_id, insert_index.or(Some(0)))
                .map_err(|error| LibraryError::Project(error.to_string()))
        })();

        if let Err(error) = result {
            project.remove_clip(clip_id);
            return Err(error);
        }

        Ok(clip_id)
    }

    pub fn remove_clip_from_track(
        project: &Arc<RwLock<Project>>,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let track = project
            .get_track(track_id)
            .ok_or_else(|| LibraryError::Project(format!("Track {track_id} not found")))?;
        if !track.clip_ids.contains(&clip_id) {
            return Err(LibraryError::Project(format!(
                "Clip {clip_id} is not in Track {track_id}"
            )));
        }
        project
            .remove_clip(clip_id)
            .map(|_| ())
            .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))
    }

    /// Atomically replace all timeline-placement fields affected by a trim
    /// gesture.  Keeping these values under one Project write lock prevents a
    /// rendered frame from observing a new start with an old trim (or vice
    /// versa).
    pub fn update_clip_timing(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
        trim_in: f64,
    ) -> Result<(), LibraryError> {
        let validate = |key, value| {
            Clip::validate_timing_property_value(key, &PropertyValue::Number(OrderedFloat(value)))
                .map_err(LibraryError::Project)
        };
        let start_time = validate(crate::model::node::CLIP_START_TIME_PROPERTY, start_time)?;
        let duration = validate(crate::model::node::CLIP_DURATION_PROPERTY, duration)?;
        let trim_in = validate(crate::model::node::CLIP_TRIM_IN_PROPERTY, trim_in)?;

        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let clip = project
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?;
        clip.start_time = OrderedFloat(start_time);
        clip.duration = OrderedFloat(duration);
        clip.trim_in = OrderedFloat(trim_in);
        Ok(())
    }

    pub fn update_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let updated = match owner {
            PropertyOwner::Clip(clip_id) => {
                let clip = project
                    .get_clip_mut(clip_id)
                    .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?;
                if Clip::timing_property_definition(property_key).is_some() {
                    if easing.is_some() {
                        return Err(LibraryError::Project(format!(
                            "Structural Clip timing property '{property_key}' cannot be keyframed"
                        )));
                    }
                    clip.update_timing_property(property_key, value)
                        .map_err(LibraryError::Project)?;
                    true
                } else {
                    clip.update_property_or_keyframe(property_key, time, value, easing)
                }
            }
            PropertyOwner::Node(node_id) => project
                .get_node_mut(node_id)
                .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?
                .update_property_or_keyframe(property_key, time, value, easing),
        };

        if updated {
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Property {property_key} could not be updated on {owner:?}"
            )))
        }
    }

    fn validate_recursion(project: &Project, child_id: Uuid, parent_id: Uuid) -> bool {
        if child_id == parent_id {
            return false;
        }

        let mut compositions = vec![child_id];
        let mut visited = HashSet::new();
        while let Some(composition_id) = compositions.pop() {
            if !visited.insert(composition_id) {
                continue;
            }
            let Some(composition) = project.get_composition(composition_id) else {
                continue;
            };

            let mut node_ids = composition.node_ids.clone();
            for track_id in &composition.track_ids {
                let Some(track) = project.get_track(*track_id) else {
                    continue;
                };
                node_ids.extend(track.node_ids.iter().copied());
                for clip_id in &track.clip_ids {
                    if let Some(clip) = project.get_clip(*clip_id) {
                        node_ids.extend(clip.node_ids.iter().copied());
                    }
                }
            }

            for node_id in node_ids {
                let Some(NodeContent::CompositionInstance(instance)) =
                    project.get_node(node_id).map(Node::content)
                else {
                    continue;
                };
                if instance.composition_id == parent_id {
                    return false;
                }
                compositions.push(instance.composition_id);
            }
        }
        true
    }

    pub fn move_clip_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
    ) -> Result<(), LibraryError> {
        Self::move_clip_to_track_at_index(
            project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_start_time,
            None,
        )
    }

    pub fn move_clip_to_track_at_index(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = project.get_composition(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition {composition_id} not found"))
        })?;
        if !composition.track_ids.contains(&source_track_id)
            || !composition.track_ids.contains(&target_track_id)
        {
            return Err(LibraryError::Project(format!(
                "Source and target Tracks must both belong to Composition {composition_id}"
            )));
        }
        let source = project
            .get_track(source_track_id)
            .ok_or_else(|| LibraryError::Project(format!("Track {source_track_id} not found")))?;
        if !source.clip_ids.contains(&clip_id) {
            return Err(LibraryError::Project(format!(
                "Clip {clip_id} is not in source Track {source_track_id}"
            )));
        }
        if project.get_track(target_track_id).is_none() {
            return Err(LibraryError::Project(format!(
                "Track {target_track_id} not found"
            )));
        }
        if project.get_clip(clip_id).is_none() {
            return Err(LibraryError::Project(format!("Clip {clip_id} not found")));
        }

        if source_track_id != target_track_id || target_index.is_some() {
            project
                .attach_clip_to_track_at(target_track_id, clip_id, target_index)
                .map_err(|error| LibraryError::Project(error.to_string()))?;
        }
        let clip = project
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} disappeared")))?;
        clip.start_time = OrderedFloat(new_start_time.max(0.0));
        Ok(())
    }

    pub fn set_property_attribute(
        project: &Arc<RwLock<Project>>,
        owner: PropertyOwner,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        set_property_attribute_value(
            &mut project,
            owner,
            property_key,
            attribute_key.to_string(),
            attribute_value,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
    use crate::model::frame::color::Color;
    use crate::model::{Composition, Track};

    fn project_with_composition(name: &str) -> (Project, Uuid, Uuid) {
        let mut project = Project::new(name);
        let (composition, track) = Composition::new(name, 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        (project, composition_id, track_id)
    }

    #[test]
    fn bundle_insertion_sets_clip_output_and_track_order_atomically() {
        let (project, composition_id, track_id) = project_with_composition("test");
        let clip = Clip::new("clip", 1.0, 2.0);
        let node = test_generator_node(
            "solid",
            GeneratorNodeRequest::Solid {
                color: Color::white(),
            },
        );
        let clip_id = clip.id;
        let node_id = node.id;
        let project = Arc::new(RwLock::new(project));

        ClipHandler::add_clip_to_track(
            &project,
            composition_id,
            track_id,
            ClipBundle::with_image_node(clip, node),
            None,
        )
        .unwrap();

        let project = project.read().unwrap();
        assert_eq!(project.get_track(track_id).unwrap().clip_ids, vec![clip_id]);
        assert_eq!(project.get_clip(clip_id).unwrap().node_ids, vec![node_id]);
        assert_eq!(
            project.get_clip(clip_id).unwrap().output_node_id,
            Some(node_id)
        );
        assert_eq!(
            project.find_node_container(node_id),
            Some(NodeContainer::Clip(clip_id))
        );
    }

    #[test]
    fn bundle_without_explicit_image_or_audio_output_is_rejected_atomically() {
        let (project, composition_id, track_id) = project_with_composition("missing output");
        let baseline = project.clone();
        let clip = Clip::new("clip", 0.0, 1.0);
        let solid = test_generator_node(
            "solid",
            GeneratorNodeRequest::Solid {
                color: Color::white(),
            },
        );
        let project = Arc::new(RwLock::new(project));
        let error = ClipHandler::add_clip_to_track(
            &project,
            composition_id,
            track_id,
            ClipBundle {
                clip,
                graph: NodeGraphBundle::new(vec![solid], Vec::new(), None),
            },
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("explicit image or audio output"));
        assert_eq!(*project.read().unwrap(), baseline);
    }

    #[test]
    fn shape_only_clip_output_is_rejected_atomically() {
        let (project, composition_id, track_id) = project_with_composition("shape output");
        let baseline = project.clone();
        let clip = Clip::new("clip", 0.0, 1.0);
        let shape = test_generator_node(
            "shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        );
        let project = Arc::new(RwLock::new(project));
        let error = ClipHandler::add_clip_to_track(
            &project,
            composition_id,
            track_id,
            ClipBundle {
                clip,
                graph: NodeGraphBundle::with_output_node(shape),
            },
            None,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not declare an image output port")
        );
        assert_eq!(*project.read().unwrap(), baseline);
    }

    #[test]
    fn recursion_validation_walks_every_top_level_track_and_clip() {
        let (mut project, parent_id, _) = project_with_composition("parent");
        let (child, child_first_track) = Composition::new("child", 1920, 1080, 30.0, 10.0);
        let child_id = child.id;
        assert!(
            project.add_track(child_first_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(child).is_ok(),
            "container structural Merge insertion must succeed"
        );

        let child_second_track = Track::new("child second");
        let child_second_track_id = child_second_track.id;
        assert!(
            project.add_track(child_second_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project
            .attach_track_to_composition(child_id, child_second_track_id)
            .unwrap();

        let instance_node = Node::new_composition_instance(
            "instance of parent",
            CompositionInstanceContent {
                composition_id: parent_id,
            },
        );
        let instance_id = instance_node.id;
        let mut instance_clip = Clip::new("composition instance", 0.0, 10.0);
        let instance_clip_id = instance_clip.id;
        instance_clip.node_ids.push(instance_id);
        instance_clip.output_node_id = Some(instance_id);
        project.add_node(instance_node);
        project.add_clip(instance_clip);
        project
            .attach_node_to_container(NodeContainer::Clip(instance_clip_id), instance_id)
            .unwrap();
        project
            .set_output_node(NodeContainer::Clip(instance_clip_id), Some(instance_id))
            .unwrap();
        project
            .attach_clip_to_track(child_second_track_id, instance_clip_id)
            .unwrap();

        assert!(!ClipHandler::validate_recursion(
            &project, child_id, parent_id
        ));
    }

    #[test]
    fn clip_timing_update_commits_start_duration_and_trim_together() {
        let (mut project, _, track_id) = project_with_composition("timing");
        let clip = Clip::new("clip", 1.0, 4.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        let project = Arc::new(RwLock::new(project));

        ClipHandler::update_clip_timing(&project, clip_id, 2.5, 2.5, 1.75).unwrap();
        let read = project.read().unwrap();
        let clip = read.get_clip(clip_id).unwrap();
        assert_eq!(clip.start_time.into_inner(), 2.5);
        assert_eq!(clip.duration.into_inner(), 2.5);
        assert_eq!(clip.trim_in.into_inner(), 1.75);
        drop(read);

        assert!(ClipHandler::update_clip_timing(&project, clip_id, 99.0, f64::NAN, 99.0,).is_err());
        let read = project.read().unwrap();
        let clip = read.get_clip(clip_id).unwrap();
        assert_eq!(clip.start_time.into_inner(), 2.5);
        assert_eq!(clip.duration.into_inner(), 2.5);
        assert_eq!(clip.trim_in.into_inner(), 1.75);
    }
}
