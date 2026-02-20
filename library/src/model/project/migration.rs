//! Migration utilities for converting embedded effects/styles/ensembles to graph nodes.

use uuid::Uuid;

use super::connection::{Connection, PinId};
use super::graph_node::GraphNode;
use super::node::Node;
use super::project::Project;

/// Migrate all embedded effects, styles, effectors, and decorators in all clips
/// to standalone GraphNodes with connections.
///
/// This is called after deserialization to convert old-format projects.
/// After migration, the embedded fields are cleared.
pub fn migrate_embedded_to_graph(project: &mut Project) {
    // Collect clip IDs and their parent tracks first (to avoid borrow issues)
    let clip_entries: Vec<(Uuid, Uuid)> = project
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            Node::Clip(_) => {
                let parent_id = project.nodes.iter().find_map(|(tid, tnode)| match tnode {
                    Node::Track(t) if t.child_ids.contains(id) => Some(*tid),
                    _ => None,
                });
                parent_id.map(|pid| (*id, pid))
            }
            _ => None,
        })
        .collect();

    for (clip_id, parent_track_id) in clip_entries {
        migrate_clip_embedded(project, clip_id, parent_track_id);
    }
}

/// Migrate embedded data from a single clip to graph nodes.
fn migrate_clip_embedded(project: &mut Project, clip_id: Uuid, parent_track_id: Uuid) {
    // Extract the embedded data from the clip
    let (effects, styles, effectors, decorators) = {
        let clip = match project.get_clip(clip_id) {
            Some(c) => c,
            None => return,
        };

        // Skip if no embedded data to migrate
        if clip.effects.is_empty()
            && clip.styles.is_empty()
            && clip.effectors.is_empty()
            && clip.decorators.is_empty()
        {
            return;
        }

        (
            clip.effects.clone(),
            clip.styles.clone(),
            clip.effectors.clone(),
            clip.decorators.clone(),
        )
    };

    // Migrate effects: create GraphNode for each, chain them via image connections
    let mut prev_image_source = PinId::new(clip_id, "image_out");
    for effect in &effects {
        let effect_node = GraphNode::new_with_id(
            effect.id,
            &format!("effect.{}", effect.effect_type),
            effect.properties.clone(),
        );
        let effect_id = effect_node.id;

        project.add_node(Node::Graph(effect_node));
        if let Some(track) = project.get_track_mut(parent_track_id) {
            track.add_child(effect_id);
        }

        // Connect: prev_image_source → effect.image_in
        project.add_connection(Connection::new(
            prev_image_source,
            PinId::new(effect_id, "image_in"),
        ));
        prev_image_source = PinId::new(effect_id, "image_out");
    }

    // Migrate styles: create GraphNode for each, connect to clip
    for style in &styles {
        let style_node = GraphNode::new_with_id(
            style.id,
            &format!("style.{}", style.style_type),
            style.properties.clone(),
        );
        let style_id = style_node.id;

        project.add_node(Node::Graph(style_node));
        if let Some(track) = project.get_track_mut(parent_track_id) {
            track.add_child(style_id);
        }

        project.add_connection(Connection::new(
            PinId::new(style_id, "style_out"),
            PinId::new(clip_id, "style_in"),
        ));
    }

    // Migrate effectors
    for effector in &effectors {
        let effector_node = GraphNode::new_with_id(
            effector.id,
            &format!("effector.{}", effector.effector_type),
            effector.properties.clone(),
        );
        let effector_id = effector_node.id;

        project.add_node(Node::Graph(effector_node));
        if let Some(track) = project.get_track_mut(parent_track_id) {
            track.add_child(effector_id);
        }

        project.add_connection(Connection::new(
            PinId::new(effector_id, "effector_out"),
            PinId::new(clip_id, "effector_in"),
        ));
    }

    // Migrate decorators
    for decorator in &decorators {
        let decorator_node = GraphNode::new_with_id(
            decorator.id,
            &format!("decorator.{}", decorator.decorator_type),
            decorator.properties.clone(),
        );
        let decorator_id = decorator_node.id;

        project.add_node(Node::Graph(decorator_node));
        if let Some(track) = project.get_track_mut(parent_track_id) {
            track.add_child(decorator_id);
        }

        project.add_connection(Connection::new(
            PinId::new(decorator_id, "decorator_out"),
            PinId::new(clip_id, "decorator_in"),
        ));
    }

    // Clear the embedded fields from the clip
    if let Some(clip) = project.get_clip_mut(clip_id) {
        clip.effects.clear();
        clip.styles.clear();
        clip.effectors.clear();
        clip.decorators.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::project::clip::{TrackClip, TrackClipKind};
    use crate::model::project::effect::EffectConfig;
    use crate::model::project::node::Node;
    use crate::model::project::property::{Property, PropertyMap, PropertyValue};
    use crate::model::project::style::StyleInstance;
    use crate::model::project::track::TrackData;
    use ordered_float::OrderedFloat;

    #[test]
    fn test_migrate_effects() {
        let mut project = Project::new("Test");
        let root_track = TrackData::new("Root");
        let root_id = root_track.id;
        project.add_node(Node::Track(root_track));

        let comp = crate::model::project::project::Composition::new_with_root(
            "c", 1920, 1080, 30.0, 10.0, root_id,
        );
        project.add_composition(comp);

        // Create clip with embedded effects
        let mut props = PropertyMap::new();
        props.set(
            "file_path".into(),
            Property::constant(PropertyValue::String("test.png".into())),
        );

        let mut effect_props = PropertyMap::new();
        effect_props.set(
            "amount".into(),
            Property::constant(PropertyValue::Number(OrderedFloat(5.0))),
        );

        let clip = TrackClip {
            id: Uuid::new_v4(),
            reference_id: None,
            kind: TrackClipKind::Image,
            in_frame: 0,
            out_frame: 100,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: props,
            styles: vec![],
            effects: vec![EffectConfig {
                id: Uuid::new_v4(),
                effect_type: "blur".to_string(),
                properties: effect_props,
            }],
            effectors: vec![],
            decorators: vec![],
        };
        let clip_id = clip.id;
        let effect_id = clip.effects[0].id;

        project.add_node(Node::Clip(clip));
        project.get_track_mut(root_id).unwrap().add_child(clip_id);

        // Before migration
        assert_eq!(project.get_clip(clip_id).unwrap().effects.len(), 1);
        assert!(project.connections.is_empty());

        // Run migration
        migrate_embedded_to_graph(&mut project);

        // After migration
        assert_eq!(project.get_clip(clip_id).unwrap().effects.len(), 0);
        assert!(!project.connections.is_empty());

        // Effect node should exist as a GraphNode
        let graph_node = project.get_graph_node(effect_id).unwrap();
        assert_eq!(graph_node.type_id, "effect.blur");
        assert!(graph_node.properties.get("amount").is_some());

        // Should have a connection: clip.image_out → effect.image_in
        assert!(project.connections.iter().any(|c| {
            c.from.node_id == clip_id
                && c.from.pin_name == "image_out"
                && c.to.node_id == effect_id
                && c.to.pin_name == "image_in"
        }));
    }

    #[test]
    fn test_migrate_styles() {
        let mut project = Project::new("Test");
        let root_track = TrackData::new("Root");
        let root_id = root_track.id;
        project.add_node(Node::Track(root_track));

        let comp = crate::model::project::project::Composition::new_with_root(
            "c", 1920, 1080, 30.0, 10.0, root_id,
        );
        project.add_composition(comp);

        let mut style_props = PropertyMap::new();
        style_props.set(
            "color".into(),
            Property::constant(PropertyValue::String("red".into())),
        );

        let clip = TrackClip {
            id: Uuid::new_v4(),
            reference_id: None,
            kind: TrackClipKind::Text,
            in_frame: 0,
            out_frame: 100,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: PropertyMap::new(),
            styles: vec![StyleInstance {
                id: Uuid::new_v4(),
                style_type: "fill".to_string(),
                properties: style_props,
            }],
            effects: vec![],
            effectors: vec![],
            decorators: vec![],
        };
        let clip_id = clip.id;
        let style_id = clip.styles[0].id;

        project.add_node(Node::Clip(clip));
        project.get_track_mut(root_id).unwrap().add_child(clip_id);

        migrate_embedded_to_graph(&mut project);

        // Style should be migrated
        assert_eq!(project.get_clip(clip_id).unwrap().styles.len(), 0);
        let style_node = project.get_graph_node(style_id).unwrap();
        assert_eq!(style_node.type_id, "style.fill");
    }

    #[test]
    fn test_no_migration_when_empty() {
        let mut project = Project::new("Test");
        let root_track = TrackData::new("Root");
        let root_id = root_track.id;
        project.add_node(Node::Track(root_track));

        let comp = crate::model::project::project::Composition::new_with_root(
            "c", 1920, 1080, 30.0, 10.0, root_id,
        );
        project.add_composition(comp);

        let clip = TrackClip {
            id: Uuid::new_v4(),
            reference_id: None,
            kind: TrackClipKind::Image,
            in_frame: 0,
            out_frame: 100,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: PropertyMap::new(),
            styles: vec![],
            effects: vec![],
            effectors: vec![],
            decorators: vec![],
        };
        let clip_id = clip.id;

        project.add_node(Node::Clip(clip));
        project.get_track_mut(root_id).unwrap().add_child(clip_id);

        let node_count_before = project.nodes.len();
        migrate_embedded_to_graph(&mut project);
        assert_eq!(project.nodes.len(), node_count_before);
        assert!(project.connections.is_empty());
    }
}
