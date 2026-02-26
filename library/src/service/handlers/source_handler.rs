use crate::error::LibraryError;
use crate::project::node::Node;
use crate::project::project::Project;
use crate::project::property::PropertyValue;
use crate::project::source::{SourceData, SourceKind};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct SourceHandler;

impl SourceHandler {
    /// Add a source to a track at a specific index (or index 0 if not specified)
    pub fn add_source_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        source: SourceData,
        in_frame: u64,
        out_frame: u64,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        // Validation: Prevent circular references if adding a composition
        if source.kind == SourceKind::Composition {
            if let Some(ref_id) = source.reference_id {
                if !Self::validate_recursion(project, ref_id, composition_id) {
                    return Err(LibraryError::project(
                        "Cannot add composition: Circular reference detected".to_string(),
                    ));
                }
            }
        }

        let mut proj = super::write_project(project)?;

        // Ensure composition exists
        let _composition = proj.get_composition(composition_id).ok_or_else(|| {
            LibraryError::project(format!("Composition with ID {} not found", composition_id))
        })?;

        // Ensure track exists and belongs to this composition's tree
        if proj.get_track(track_id).is_none() {
            return Err(LibraryError::project(format!(
                "Track with ID {} not found",
                track_id
            )));
        }
        if !proj.is_node_in_tree(composition_id, track_id) {
            return Err(LibraryError::project(format!(
                "Track {} does not belong to composition {}",
                track_id, composition_id
            )));
        }

        let source_id = source.id;
        let mut final_source = source;
        final_source.in_frame = in_frame;
        final_source.out_frame = out_frame;

        // Add source to nodes registry
        proj.add_node(Node::Source(final_source));

        // Add source ID to track's children at specified index (or 0 for top of layer list)
        if let Some(track) = proj.get_track_mut(track_id) {
            let idx = insert_index.unwrap_or(0);
            track.insert_child(idx, source_id);
        }

        Ok(source_id)
    }

    /// Create a layer container with default graph nodes for a newly added source.
    ///
    /// Creates a `Node::Layer` containing the source and its graph nodes:
    /// - For text/shape: `source.shape_out → fill.shape_in`, `fill.image_out → transform.image_in`
    /// - For video/image/sksl: `source.image_out → transform.image_in`
    /// - Non-audio: `transform.image_out → layer.image_out` (container output connection)
    /// - Audio: layer container only (no graph nodes)
    pub fn setup_source_graph_nodes(
        project: &Arc<RwLock<Project>>,
        plugin_manager: &crate::plugin::PluginManager,
        track_id: Uuid,
        source_id: Uuid,
        source_kind: &SourceKind,
    ) -> Result<(), LibraryError> {
        use crate::project::connection::PinId;
        use crate::project::layer::LayerData;

        log::info!(
            "[SetupGraph] source={} kind={:?} track={}",
            source_id,
            source_kind,
            track_id
        );

        // 1. Create a Layer container and move the source into it
        let layer_id = {
            let mut proj = super::write_project(project)?;

            // Read timing from source
            let (in_frame, out_frame) = proj
                .get_source(source_id)
                .map(|s| (s.in_frame, s.out_frame))
                .unwrap_or((0, 0));

            let mut layer = LayerData::new("Layer", in_frame, out_frame);
            let layer_id = layer.id;

            // Move source from parent track to layer
            if let Some(parent_track) = proj.get_track_mut(track_id) {
                parent_track.remove_child(source_id);
                parent_track.add_child(layer_id);
            }
            layer.add_child(source_id);
            proj.add_node(Node::Layer(layer));

            log::debug!(
                "[SetupGraph] Created layer {} (in={}, out={})",
                layer_id,
                in_frame,
                out_frame
            );
            layer_id
        };

        // Audio sources get a layer container but no graph nodes
        if *source_kind == SourceKind::Audio {
            log::debug!(
                "[SetupGraph] Audio source {} wrapped in layer {}",
                source_id,
                layer_id
            );
            return Ok(());
        }

        // 2. Create compositing.transform node inside the layer
        let transform_id = super::graph_handler::GraphHandler::add_graph_node(
            project,
            plugin_manager,
            layer_id,
            "compositing.transform",
        )?;
        log::debug!("[SetupGraph] Created transform {}", transform_id);

        // 3. Create connections based on source kind
        if *source_kind == SourceKind::Text || *source_kind == SourceKind::Shape {
            // Text/Shape: source.shape_out → fill.shape_in → fill.image_out → transform.image_in
            let fill_id = super::graph_handler::GraphHandler::add_graph_node(
                project,
                plugin_manager,
                layer_id,
                "style.fill",
            )?;
            log::debug!("[SetupGraph] Created fill {}", fill_id);

            super::graph_handler::GraphHandler::add_connection(
                project,
                PinId::new(source_id, "shape_out"),
                PinId::new(fill_id, "shape_in"),
            )?;
            log::debug!("[SetupGraph] Connected source.shape_out -> fill.shape_in");

            super::graph_handler::GraphHandler::add_connection(
                project,
                PinId::new(fill_id, "image_out"),
                PinId::new(transform_id, "image_in"),
            )?;
            log::debug!("[SetupGraph] Connected fill.image_out -> transform.image_in");
        } else {
            // Video/Image/SkSL: source.image_out → transform.image_in
            super::graph_handler::GraphHandler::add_connection(
                project,
                PinId::new(source_id, "image_out"),
                PinId::new(transform_id, "image_in"),
            )?;
            log::debug!("[SetupGraph] Connected source.image_out -> transform.image_in");
        }

        // 4. Container output: transform.image_out → layer.image_out
        super::graph_handler::GraphHandler::add_connection(
            project,
            PinId::new(transform_id, "image_out"),
            PinId::new(layer_id, "image_out"),
        )?;
        log::debug!("[SetupGraph] Connected transform.image_out -> layer.image_out");
        log::debug!("[SetupGraph] Setup complete for source {}", source_id);

        Ok(())
    }

    /// Remove a layer (source + container) from a track, along with all associated graph nodes
    /// (transform, effects, styles, effectors, decorators).
    ///
    /// Finds the Layer container that wraps the source, removes all graph nodes within it,
    /// then removes the Layer container itself from its parent track.
    pub fn remove_source_from_track(
        project: &Arc<RwLock<Project>>,
        track_id: Uuid,
        source_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        // 1. Collect all associated graph nodes before removing anything
        let associated_nodes =
            crate::project::graph_analysis::collect_all_associated_nodes(&proj, source_id);

        // 2. Remove associated graph nodes (connections are cleaned up per node)
        for node_id in &associated_nodes {
            // Remove from parent container's child_ids (Track or Layer)
            let parent_ids: Vec<Uuid> = proj
                .nodes
                .iter()
                .filter_map(|(id, n)| match n {
                    Node::Track(t) if t.child_ids.contains(node_id) => Some(*id),
                    Node::Layer(l) if l.child_ids.contains(node_id) => Some(*id),
                    _ => None,
                })
                .collect();

            for pid in parent_ids {
                if let Some(children) = proj.get_container_child_ids_mut(pid) {
                    children.retain(|id| id != node_id);
                }
            }
            proj.remove_connections_for_node(*node_id);
            proj.remove_node(*node_id);
        }

        // 3. Find the Layer container that holds this source
        let layer_id = proj.find_parent_track(source_id);

        // 4. Remove source connections and node
        if let Some(lid) = layer_id {
            if let Some(children) = proj.get_container_child_ids_mut(lid) {
                children.retain(|id| *id != source_id);
            }
        }
        proj.remove_connections_for_node(source_id);
        proj.remove_node(source_id);

        // 5. Remove the Layer container itself from its parent track
        if let Some(lid) = layer_id {
            if proj.get_layer(lid).is_some() {
                // Remove Layer from parent track's child_ids
                if let Some(track) = proj.get_track_mut(track_id) {
                    track.remove_child(lid);
                }
                proj.remove_connections_for_node(lid);
                proj.remove_node(lid);
            }
        }

        Ok(())
    }

    /// Unified method to update property or keyframe for any target
    pub fn update_target_property_or_keyframe(
        project: &Arc<RwLock<Project>>,
        source_id: Uuid,
        target: crate::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        // GraphNode targets are accessed via Project.nodes, not via source
        if let crate::project::property::PropertyTarget::GraphNode(node_id) = target {
            let mut proj = super::write_project(project)?;
            let node = proj.get_graph_node_mut(node_id).ok_or_else(|| {
                LibraryError::project(format!("Graph node {} not found", node_id))
            })?;
            node.properties
                .update_property_or_keyframe(property_key, time, value, easing);
            return Ok(());
        }

        let mut proj = super::write_project(project)?;

        let source = proj.get_source_mut(source_id).ok_or_else(|| {
            LibraryError::project(format!("Source with ID {} not found", source_id))
        })?;

        // Special handling for Source struct fields sync
        if let crate::project::property::PropertyTarget::Clip = target {
            match property_key {
                "in_frame" => {
                    if let PropertyValue::Number(n) = &value {
                        source.in_frame = n.into_inner().round() as u64;
                    }
                }
                "out_frame" => {
                    if let PropertyValue::Number(n) = &value {
                        source.out_frame = n.into_inner().round() as u64;
                    }
                }
                "source_begin_frame" => {
                    if let PropertyValue::Number(n) = &value {
                        source.source_begin_frame = n.into_inner().round() as i64;
                    }
                }
                _ => {}
            }
        }

        let prop_map = source
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

            if let Some(comp) = project_read.get_composition(current_id) {
                // Collect all sources from the composition's children
                for source in project_read.collect_sources(comp.id) {
                    if source.kind == SourceKind::Composition {
                        if let Some(ref_id) = source.reference_id {
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

    pub fn move_source_to_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        source_track_id: Uuid,
        source_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
    ) -> Result<(), LibraryError> {
        Self::move_source_to_track_at_index(
            project,
            composition_id,
            source_track_id,
            source_id,
            target_track_id,
            new_in_frame,
            None,
        )
    }

    pub fn move_source_to_track_at_index(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        source_track_id: Uuid,
        source_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        // Find the Layer container wrapping this source
        let layer_id = proj.find_parent_track(source_id);

        // The moveable unit is the layer container (if it exists), not the raw source
        let move_id = layer_id.unwrap_or(source_id);

        // 1. Remove from source track's child_ids
        if let Some(src_track) = proj.get_track_mut(source_track_id) {
            if !src_track.remove_child(move_id) {
                return Err(LibraryError::project(format!(
                    "Source/Layer {} not found in source track",
                    move_id
                )));
            }
        } else {
            return Err(LibraryError::project(format!(
                "Source track {} not found",
                source_track_id
            )));
        }

        // 2. Update source timing and sync Layer timing
        if let Some(source) = proj.get_source_mut(source_id) {
            let duration = source.out_frame - source.in_frame;
            source.in_frame = new_in_frame;
            source.out_frame = new_in_frame + duration;
        }
        if let Some(lid) = layer_id {
            if let Some(layer) = proj.get_layer_mut(lid) {
                let duration = layer.out_frame - layer.in_frame;
                layer.in_frame = new_in_frame;
                layer.out_frame = new_in_frame + duration;
            }
        }

        // 3. Add to target track's child_ids
        if let Some(target_track) = proj.get_track_mut(target_track_id) {
            if let Some(idx) = target_index {
                target_track.insert_child(idx, move_id);
            } else {
                target_track.add_child(move_id);
            }
        } else {
            return Err(LibraryError::project(format!(
                "Target track {} not found",
                target_track_id
            )));
        }

        Ok(())
    }

    pub fn set_property_attribute(
        project: &Arc<RwLock<Project>>,
        source_id: Uuid,
        target: crate::project::property::PropertyTarget,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        // GraphNode targets are accessed via Project.nodes, not via source
        if let crate::project::property::PropertyTarget::GraphNode(node_id) = target {
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

        let source = proj.get_source_mut(source_id).ok_or_else(|| {
            LibraryError::project(format!("Source with ID {} not found", source_id))
        })?;

        let prop_map = source.get_property_map_mut(target).ok_or_else(|| {
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
