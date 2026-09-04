use crate::state::context_types::{PreviewEditTarget, SelectionTarget};
use library::model::Node;
use library::model::frame::entity::{FrameGroupKind, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::frame::transform::Transform;
use library::model::project::Project;
use library::rendering::renderer::Affine2D;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewSpatialKind {
    Content,
    ShapeTransform,
    ImageTransform,
}

#[derive(Clone)]
pub struct PreviewSpatialLayer {
    pub node: Node,
    pub kind: PreviewSpatialKind,
    /// Direct transform owned by this Node, excluding every outer layer.
    pub transform: Transform,
    /// Affine stack outside this layer. Pointer deltas are mapped through its
    /// inverse before writing this Node's direct Project properties.
    pub parent_transform: Affine2D,
}

impl PreviewSpatialLayer {
    fn is_editable(&self) -> bool {
        ["position", "rotation", "scale", "anchor"]
            .into_iter()
            .all(|key| self.node.properties().get(key).is_some())
    }
}

/// One interactive visual that actually reached the rendered Composition.
///
/// This is an ephemeral UI projection of `FrameInfo`, not a second project
/// model. Its identity and ordering come from authoritative frame evaluation;
/// wrapper Nodes such as Style, Effect, and Merge never replace the authored
/// geometry Node or its optional spatial Transform owner.
pub struct PreviewClip {
    /// Generator that owns the rendered content (text, path, media, shader).
    pub content_node: Node,
    /// Ordered outer-to-inner spatial provenance. Image Transform groups are
    /// explicit layers; the innermost entry is the Shape/content placement.
    pub spatial_layers: Vec<PreviewSpatialLayer>,
    /// Nearest authoritative Timeline/Inspector owner for a Preview hit.
    pub owner_target: SelectionTarget,
    /// Final evaluated visual transform (normalized scale/opacity). This may
    /// include downstream Shape Effector contributions and is used for hit
    /// testing/drawing only.
    pub transform: Transform,
    /// Every evaluated group/layer transform composed with the content.
    pub world_transform: Affine2D,
    pub content_bounds: Option<(f32, f32, f32, f32)>,
    /// Stable render-branch identity. Project selection remains the source
    /// Node ID, while this path distinguishes fan-out of that Node through
    /// multiple Merge/Composition Instance branches.
    pub instance_path: Vec<Uuid>,
}

impl PreviewClip {
    pub fn content_id(&self) -> Uuid {
        self.content_node.id
    }

    pub fn spatial_id(&self) -> Option<Uuid> {
        self.editable_spatial_id()
    }

    /// A stale/malformed spatial owner is never made draggable. Every Preview
    /// spatial gesture writes this complete native property contract.
    pub fn editable_spatial_id(&self) -> Option<Uuid> {
        self.spatial_layers
            .iter()
            .find(|layer| layer.is_editable())
            .map(|layer| layer.node.id)
    }

    pub fn matches_node_id(&self, node_id: Uuid) -> bool {
        self.content_id() == node_id
            || self
                .spatial_layers
                .iter()
                .any(|layer| layer.node.id == node_id)
    }

    pub fn spatial_layer(&self, node_id: Uuid) -> Option<&PreviewSpatialLayer> {
        self.spatial_layers
            .iter()
            .find(|layer| layer.node.id == node_id && layer.is_editable())
    }

    pub fn edit_target(&self) -> PreviewEditTarget {
        PreviewEditTarget {
            owner: self.owner_target,
            content_node_id: self.content_id(),
            spatial_node_id: self.editable_spatial_id(),
            instance_path: self.instance_path.clone(),
        }
    }
}

pub fn from_evaluated_frame(project: &Project, frame: &FrameInfo) -> Vec<PreviewClip> {
    let mut visuals = Vec::new();
    let mut path = Vec::new();
    let mut image_layers = Vec::new();
    for item in &frame.items {
        collect_visuals(
            project,
            item,
            Affine2D::IDENTITY,
            &mut path,
            &mut image_layers,
            &mut visuals,
        );
    }
    visuals
}

/// Resolve exactly one rendered branch for either its content or spatial Node.
///
/// Explicit Preview interaction state must use this lookup. A stale path is a
/// stale identity and deliberately does not fall back to a different rendered
/// instance of the same Project Node.
pub fn visual_for_exact_instance<'a>(
    visuals: &'a [PreviewClip],
    node_id: Uuid,
    instance_path: &[Uuid],
) -> Option<&'a PreviewClip> {
    visuals
        .iter()
        .find(|visual| visual.matches_node_id(node_id) && visual.instance_path == instance_path)
}

/// Resolve the top-most rendered branch for a Project Node.
///
/// This is the intentional default for selection arriving from a panel that
/// has no rendered branch identity, such as Timeline or Inspector.
#[cfg(test)]
pub fn topmost_visual_for_node(visuals: &[PreviewClip], node_id: Uuid) -> Option<&PreviewClip> {
    visuals
        .iter()
        .rev()
        .find(|visual| visual.matches_node_id(node_id))
}

/// Compatibility dispatcher for callers that may or may not carry a branch.
///
/// `Some(path)` has exact-only semantics; only `None` opts into the documented
/// top-most default.
#[cfg(test)]
pub fn visual_for_selection<'a>(
    visuals: &'a [PreviewClip],
    node_id: Uuid,
    instance_path: Option<&[Uuid]>,
) -> Option<&'a PreviewClip> {
    match instance_path {
        Some(path) => visual_for_exact_instance(visuals, node_id, path),
        None => topmost_visual_for_node(visuals, node_id),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerEditTargetResolution {
    Resolved(PreviewEditTarget),
    Ambiguous { candidate_node_ids: Vec<Uuid> },
    Unavailable,
}

/// Resolve the canonical facade edit behind a Timeline/Inspector owner.
///
/// Candidate precedence is Image Transform, Shape Transform, then Content.
/// Within one kind, layers are considered outer-to-inner in authoritative
/// render order, but a layer is eligible only when it is present in *every*
/// visual owned by the facade. This lets differing outer wrappers converge on
/// a common inner Transform. Multiple independent candidates are deliberately
/// ambiguous: Timeline-only editing must never mutate an arbitrary front-most
/// Node. A direct Preview hit may still choose one exact branch explicitly.
pub fn resolve_owner_edit_target(
    visuals: &[PreviewClip],
    owner: SelectionTarget,
) -> OwnerEditTargetResolution {
    let owned = visuals
        .iter()
        .filter(|visual| visual.owner_target == owner)
        .collect::<Vec<_>>();
    if owned.is_empty() {
        return OwnerEditTargetResolution::Unavailable;
    }

    for kind in [
        PreviewSpatialKind::ImageTransform,
        PreviewSpatialKind::ShapeTransform,
        PreviewSpatialKind::Content,
    ] {
        let Some(shared) = shared_editable_candidate(&owned, kind) else {
            continue;
        };
        let Some(visual) = owned
            .iter()
            .rev()
            .find(|visual| visual.spatial_layer(shared).is_some())
        else {
            return OwnerEditTargetResolution::Unavailable;
        };
        return OwnerEditTargetResolution::Resolved(PreviewEditTarget {
            owner,
            content_node_id: visual.content_id(),
            spatial_node_id: Some(shared),
            instance_path: visual.instance_path.clone(),
        });
    }

    let mut candidate_node_ids = owned
        .iter()
        .flat_map(|visual| {
            visual
                .spatial_layers
                .iter()
                .filter(|layer| layer.is_editable())
                .map(|layer| layer.node.id)
        })
        .collect::<Vec<_>>();
    candidate_node_ids.sort_unstable();
    candidate_node_ids.dedup();
    if candidate_node_ids.is_empty() {
        OwnerEditTargetResolution::Unavailable
    } else if owned.len() == 1 {
        OwnerEditTargetResolution::Resolved(owned[0].edit_target())
    } else {
        OwnerEditTargetResolution::Ambiguous { candidate_node_ids }
    }
}

fn shared_editable_candidate(owned: &[&PreviewClip], kind: PreviewSpatialKind) -> Option<Uuid> {
    let first = owned.first()?;
    first
        .spatial_layers
        .iter()
        .filter(|layer| layer.kind == kind && layer.is_editable())
        .map(|layer| layer.node.id)
        .find(|candidate| {
            owned.iter().skip(1).all(|visual| {
                visual.spatial_layers.iter().any(|layer| {
                    layer.node.id == *candidate && layer.kind == kind && layer.is_editable()
                })
            })
        })
}

fn collect_visuals(
    project: &Project,
    item: &FrameItem,
    parent_transform: Affine2D,
    path: &mut Vec<Uuid>,
    image_layers: &mut Vec<PreviewSpatialLayer>,
    visuals: &mut Vec<PreviewClip>,
) {
    match item {
        FrameItem::Object(object) => {
            let Some(content_node) = project.get_node(object.source_node_id) else {
                // A render result can arrive immediately after a Project
                // replacement. Never make stale frame identity interactive.
                return;
            };
            let shape_spatial_node = object
                .spatial_transform_node_id
                .and_then(|node_id| project.get_node(node_id))
                .cloned();
            let transform = object.content.transform().clone();
            path.push(object.source_node_id);
            let distinct_spatial_id = object
                .spatial_transform_node_id
                .filter(|node_id| *node_id != object.source_node_id);
            if let Some(node_id) = distinct_spatial_id {
                path.push(node_id);
            }
            let mut spatial_layers = image_layers.clone();
            if let Some(node) = shape_spatial_node {
                spatial_layers.push(PreviewSpatialLayer {
                    kind: if node.id == object.source_node_id {
                        PreviewSpatialKind::Content
                    } else {
                        PreviewSpatialKind::ShapeTransform
                    },
                    node,
                    transform: object.spatial_transform.as_ref().clone(),
                    parent_transform,
                });
            }
            let editable_spatial_id = spatial_layers
                .iter()
                .find(|layer| layer.is_editable())
                .map(|layer| layer.node.id);
            if let Some(owner_target) =
                visual_owner_target(project, editable_spatial_id, object.source_node_id)
            {
                visuals.push(PreviewClip {
                    content_node: content_node.clone(),
                    spatial_layers,
                    owner_target,
                    world_transform: parent_transform.compose(Affine2D::from(&transform)),
                    transform,
                    content_bounds: object.content_bounds.map(|bounds| bounds.as_tuple()),
                    instance_path: path.clone(),
                });
            }
            if distinct_spatial_id.is_some() {
                path.pop();
            }
            path.pop();
        }
        FrameItem::Group(group) => {
            path.push(group.source_id);
            let mut pushed_image_layer = false;
            if group.kind == FrameGroupKind::ImageTransform {
                if let Some(node) = project.get_node(group.source_id).cloned() {
                    image_layers.push(PreviewSpatialLayer {
                        node,
                        kind: PreviewSpatialKind::ImageTransform,
                        transform: group.transform.clone(),
                        parent_transform,
                    });
                    pushed_image_layer = true;
                }
            }
            let transform = parent_transform.compose(Affine2D::from(&group.transform));
            for child in &group.items {
                collect_visuals(project, child, transform, path, image_layers, visuals);
            }
            if pushed_image_layer {
                image_layers.pop();
            }
            path.pop();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthoritativeOwner {
    Unowned,
    Unique(SelectionTarget),
    Ambiguous,
}

pub(super) fn visual_owner_target(
    project: &Project,
    editable_spatial_id: Option<Uuid>,
    content_node_id: Uuid,
) -> Option<SelectionTarget> {
    let spatial_owner = editable_spatial_id.map_or(AuthoritativeOwner::Unowned, |node_id| {
        authoritative_owner_for_node(project, node_id)
    });
    let content_owner = authoritative_owner_for_node(project, content_node_id);

    // Any duplicate containment is corrupt identity and must not create an
    // interactive Preview visual. Otherwise the outermost editable spatial
    // layer owns selection; detached wrappers inherit the uniquely contained
    // content facade instead of pretending to be a standalone Node owner.
    match (spatial_owner, content_owner) {
        (AuthoritativeOwner::Ambiguous, _) | (_, AuthoritativeOwner::Ambiguous) => None,
        (AuthoritativeOwner::Unique(owner), _) => Some(owner),
        (AuthoritativeOwner::Unowned, AuthoritativeOwner::Unique(owner)) => Some(owner),
        (AuthoritativeOwner::Unowned, AuthoritativeOwner::Unowned) => None,
    }
}

fn authoritative_owner_for_node(project: &Project, node_id: Uuid) -> AuthoritativeOwner {
    let owners = project
        .compositions
        .iter()
        .filter(|composition| composition.node_ids.contains(&node_id))
        .map(|composition| SelectionTarget::Composition(composition.id))
        .chain(
            project
                .tracks
                .values()
                .filter(|track| track.node_ids.contains(&node_id))
                .map(|track| SelectionTarget::Track(track.id)),
        )
        .chain(
            project
                .clips
                .values()
                .filter(|clip| clip.node_ids.contains(&node_id))
                .map(|clip| SelectionTarget::Clip(clip.id)),
        );

    let mut unique_owner = None;
    for owner in owners {
        if unique_owner.replace(owner).is_some() {
            return AuthoritativeOwner::Ambiguous;
        }
    }
    match unique_owner {
        Some(owner) => AuthoritativeOwner::Unique(owner),
        None => AuthoritativeOwner::Unowned,
    }
}

#[cfg(test)]
include!("clip_tests.rs");
