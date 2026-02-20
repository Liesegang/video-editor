use crate::error::LibraryError;
use crate::model::project::clip::{TrackClip, TrackClipKind};
use crate::model::project::node::Node;
use crate::model::project::project::Project;
use crate::model::project::property::PropertyValue;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct ClipHandler;

impl ClipHandler {
    /// Add a clip to a track at a specific index (or index 0 if not specified)
    pub fn add_clip_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        clip: TrackClip,
        in_frame: u64,
        out_frame: u64,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        // Validation: Prevent circular references if adding a composition
        if clip.kind == TrackClipKind::Composition {
            if let Some(ref_id) = clip.reference_id {
                if !Self::validate_recursion(project, ref_id, composition_id) {
                    return Err(LibraryError::project(
                        "Cannot add composition: Circular reference detected".to_string(),
                    ));
                }
            }
        }

        let mut proj = super::write_project(project)?;

        // Ensure track exists
        if proj.get_track(track_id).is_none() {
            return Err(LibraryError::project(format!(
                "Track with ID {} not found",
                track_id
            )));
        }

        let clip_id = clip.id;
        let mut final_clip = clip;
        final_clip.in_frame = in_frame;
        final_clip.out_frame = out_frame;

        // Add clip to nodes registry
        proj.add_node(Node::Clip(final_clip));

        // Add clip ID to track's children at specified index (or 0 for top of layer list)
        if let Some(track) = proj.get_track_mut(track_id) {
            let idx = insert_index.unwrap_or(0);
            track.insert_child(idx, clip_id);
        }

        Ok(clip_id)
    }

    /// Remove a clip from a track
    pub fn remove_clip_from_track(
        project: &Arc<RwLock<Project>>,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        // Remove from parent track's child_ids
        if let Some(track) = proj.get_track_mut(track_id) {
            if !track.remove_child(clip_id) {
                return Err(LibraryError::project(format!(
                    "Clip {} not found in track {}",
                    clip_id, track_id
                )));
            }
        } else {
            return Err(LibraryError::project(format!(
                "Track {} not found",
                track_id
            )));
        }

        // Remove from nodes registry
        proj.remove_node(clip_id);
        Ok(())
    }

    /// Unified method to update property or keyframe for any target
    pub fn update_target_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        // GraphNode targets are accessed via Project.nodes, not via clip
        if let crate::model::project::property::PropertyTarget::GraphNode(node_id) = target {
            let mut proj = super::write_project(project)?;
            let node = proj.get_graph_node_mut(node_id).ok_or_else(|| {
                LibraryError::project(format!("Graph node {} not found", node_id))
            })?;
            node.properties
                .update_property_or_keyframe(property_key, time, value, easing);
            return Ok(());
        }

        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project(format!("Clip with ID {} not found", clip_id)))?;

        // Special handling for Clip struct fields sync
        if let crate::model::project::property::PropertyTarget::Clip = target {
            match property_key {
                "in_frame" => {
                    if let PropertyValue::Number(n) = &value {
                        clip.in_frame = n.into_inner().round() as u64;
                    }
                }
                "out_frame" => {
                    if let PropertyValue::Number(n) = &value {
                        clip.out_frame = n.into_inner().round() as u64;
                    }
                }
                "source_begin_frame" => {
                    if let PropertyValue::Number(n) = &value {
                        clip.source_begin_frame = n.into_inner().round() as i64;
                    }
                }
                _ => {}
            }
        }

        let prop_map = clip
            .get_property_map_mut(target.clone())
            .ok_or_else(|| LibraryError::project(format!("Target {:?} not found", target)))?;

        prop_map.update_property_or_keyframe(property_key, time, value, easing);

        Ok(())
    }

    fn validate_recursion(project: &Arc<RwLock<Project>>, child_id: Uuid, parent_id: Uuid) -> bool {
        if child_id == parent_id {
            return false;
        }
        let project_read = match project.read() {
            Ok(p) => p,
            Err(_) => return false,
        };

        let mut stack = vec![child_id];
        let mut visited = std::collections::HashSet::new();

        while let Some(current_id) = stack.pop() {
            if !visited.insert(current_id) {
                continue;
            }

            if let Some(comp) = project_read
                .compositions
                .iter()
                .find(|c| c.id == current_id)
            {
                // Collect all clips from the root track
                for clip in project_read.collect_clips(comp.root_track_id) {
                    if clip.kind == TrackClipKind::Composition {
                        if let Some(ref_id) = clip.reference_id {
                            if ref_id == parent_id {
                                return false;
                            }
                            stack.push(ref_id);
                        }
                    }
                }
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
        new_in_frame: u64,
    ) -> Result<(), LibraryError> {
        Self::move_clip_to_track_at_index(
            project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_in_frame,
            None,
        )
    }

    pub fn move_clip_to_track_at_index(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        // 1. Remove from source track's child_ids
        if let Some(source_track) = proj.get_track_mut(source_track_id) {
            if !source_track.remove_child(clip_id) {
                return Err(LibraryError::project(format!(
                    "Clip {} not found in source track",
                    clip_id
                )));
            }
        } else {
            return Err(LibraryError::project(format!(
                "Source track {} not found",
                source_track_id
            )));
        }

        // 2. Update clip timing
        if let Some(clip) = proj.get_clip_mut(clip_id) {
            let duration = clip.out_frame - clip.in_frame;
            clip.in_frame = new_in_frame;
            clip.out_frame = new_in_frame + duration;
        }

        // 3. Add to target track's child_ids
        if let Some(target_track) = proj.get_track_mut(target_track_id) {
            if let Some(idx) = target_index {
                target_track.insert_child(idx, clip_id);
            } else {
                target_track.add_child(clip_id);
            }
        } else {
            return Err(LibraryError::project(format!(
                "Target track {} not found",
                target_track_id
            )));
        }

        Ok(())
    }

    pub fn add_effect(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        effect: crate::model::project::effect::EffectConfig,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project("Clip not found".to_string()))?;

        clip.effects.push(effect);
        Ok(())
    }

    pub fn update_effects(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        effects: Vec<crate::model::project::effect::EffectConfig>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project("Clip not found".to_string()))?;

        clip.effects = effects;
        Ok(())
    }

    pub fn update_styles(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        styles: Vec<crate::model::project::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project("Clip not found".to_string()))?;

        clip.styles = styles;
        Ok(())
    }

    pub fn update_effectors(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        effectors: Vec<crate::model::project::ensemble::EffectorInstance>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project("Clip not found".to_string()))?;

        clip.effectors = effectors;
        Ok(())
    }

    pub fn update_decorators(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        decorators: Vec<crate::model::project::ensemble::DecoratorInstance>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project("Clip not found".to_string()))?;

        clip.decorators = decorators;
        Ok(())
    }

    pub fn set_property_attribute(
        project: &Arc<RwLock<Project>>,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        // GraphNode targets are accessed via Project.nodes, not via clip
        if let crate::model::project::property::PropertyTarget::GraphNode(node_id) = target {
            let mut proj = super::write_project(project)?;
            let node = proj.get_graph_node_mut(node_id).ok_or_else(|| {
                LibraryError::project(format!("Graph node {} not found", node_id))
            })?;
            let prop = node.properties.get_mut(property_key).ok_or_else(|| {
                LibraryError::project(format!("Property {} not found", property_key))
            })?;
            prop.properties
                .insert(attribute_key.to_string(), attribute_value);
            return Ok(());
        }

        let mut proj = super::write_project(project)?;

        let clip = proj
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::project(format!("Clip with ID {} not found", clip_id)))?;

        let prop_map = clip.get_property_map_mut(target).ok_or_else(|| {
            LibraryError::project("Target not found or index out of range".to_string())
        })?;

        let prop = prop_map
            .get_mut(property_key)
            .ok_or_else(|| LibraryError::project(format!("Property {} not found", property_key)))?;

        prop.properties
            .insert(attribute_key.to_string(), attribute_value);
        Ok(())
    }
}
