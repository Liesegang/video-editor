use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct ReparentContainerGeometry {
    pub(super) container: NodeContainer,
    pub(super) visible_rect: egui::Rect,
    pub(super) content_rect: Option<egui::Rect>,
    pub(super) depth: u8,
    pub(super) stacking_order: usize,
    pub(super) collapsed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReparentTargetKind {
    Geometry,
    RetainedOrigin,
    RootFallback,
}

impl ReparentTargetKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Geometry => "geometry",
            Self::RetainedOrigin => "retained_origin",
            Self::RootFallback => "root_fallback",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ReparentTargetEvaluation {
    pub(super) container: NodeContainer,
    pub(super) visible_rect: egui::Rect,
    pub(super) content_rect: egui::Rect,
    pub(super) depth: u8,
    pub(super) stacking_order: usize,
    pub(super) overlap_ratio: f32,
    pub(super) center_inside: bool,
    pub(super) pointer_inside: bool,
    pub(super) root_fallback: bool,
    pub(super) score: f32,
    pub(super) kind: ReparentTargetKind,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NodeDropIntent {
    pub(super) node_id: Uuid,
    pub(super) final_rect: egui::Rect,
    pub(super) target: ReparentTargetEvaluation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReparentReleaseOutcome {
    NoIntent,
    Applied,
    Rejected,
}

pub(super) fn reparent_container_geometries(
    project: &Project,
    composition_id: Uuid,
) -> Vec<ReparentContainerGeometry> {
    let Some(composition) = project.get_composition(composition_id) else {
        return Vec::new();
    };
    let composition_rect = container_rect(composition.ui_position, composition.ui_size);
    let mut geometries = vec![ReparentContainerGeometry {
        container: NodeContainer::Composition(composition_id),
        visible_rect: if composition.ui_collapsed {
            egui::Rect::from_min_size(
                composition_rect.min,
                egui::vec2(composition_rect.width(), CONTAINER_HEADER_HEIGHT),
            )
        } else {
            composition_rect
        },
        content_rect: (!composition.ui_collapsed)
            .then(|| composition_content_rect(composition_rect)),
        depth: 0,
        stacking_order: 0,
        collapsed: composition.ui_collapsed,
    }];
    if composition.ui_collapsed {
        return geometries;
    }

    let mut stacking_order = 1;
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        let visible_rect = if track.ui_collapsed {
            egui::Rect::from_min_size(
                track_rect.min,
                egui::vec2(track_rect.width(), CONTAINER_HEADER_HEIGHT),
            )
        } else {
            track_rect
        };
        geometries.push(ReparentContainerGeometry {
            container: NodeContainer::Track(*track_id),
            visible_rect,
            content_rect: (!track.ui_collapsed)
                .then(|| nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP)),
            depth: 1,
            stacking_order,
            collapsed: track.ui_collapsed,
        });
        stacking_order += 1;
        if track.ui_collapsed {
            continue;
        }
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            let visible_rect = if clip.ui_collapsed {
                egui::Rect::from_min_size(
                    clip_rect.min,
                    egui::vec2(clip_rect.width(), CONTAINER_HEADER_HEIGHT),
                )
            } else {
                clip_rect
            };
            geometries.push(ReparentContainerGeometry {
                container: NodeContainer::Clip(*clip_id),
                visible_rect,
                content_rect: (!clip.ui_collapsed)
                    .then(|| nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP)),
                depth: 2,
                stacking_order,
                collapsed: clip.ui_collapsed,
            });
            stacking_order += 1;
        }
    }
    geometries
}

pub(super) fn rect_area(rect: egui::Rect) -> f32 {
    if rect.is_positive() {
        rect.width() * rect.height()
    } else {
        0.0
    }
}

pub(super) fn overlap_ratio(outer: egui::Rect, inner: egui::Rect) -> f32 {
    let inner_area = rect_area(inner);
    if inner_area <= f32::EPSILON {
        return 0.0;
    }
    (rect_area(outer.intersect(inner)) / inner_area).clamp(0.0, 1.0)
}

pub(super) fn evaluate_reparent_target(
    geometry: ReparentContainerGeometry,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let content_rect = geometry.content_rect?;
    if !content_rect.is_positive() || !node_rect.is_positive() {
        return None;
    }
    let overlap_ratio = overlap_ratio(content_rect, node_rect);
    let center_inside = content_rect.contains(node_rect.center());
    let pointer_inside = content_rect.contains(drop_point);
    let eligible =
        center_inside || pointer_inside && overlap_ratio >= NODE_REPARENT_POINTER_OVERLAP_THRESHOLD;
    if !eligible {
        return None;
    }
    let score = f32::from(geometry.depth) * 10_000.0
        + if center_inside { 1_000.0 } else { 0.0 }
        + if pointer_inside { 500.0 } else { 0.0 }
        + overlap_ratio * 100.0
        + geometry.stacking_order as f32 * 0.001;
    Some(ReparentTargetEvaluation {
        container: geometry.container,
        visible_rect: geometry.visible_rect,
        content_rect,
        depth: geometry.depth,
        stacking_order: geometry.stacking_order,
        overlap_ratio,
        center_inside,
        pointer_inside,
        root_fallback: false,
        score,
        kind: ReparentTargetKind::Geometry,
    })
}

pub(super) fn deepest_legal_reparent_target(
    project: &Project,
    composition_id: Uuid,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let geometries = reparent_container_geometries(project, composition_id);
    let selected = geometries
        .iter()
        .copied()
        .filter_map(|geometry| evaluate_reparent_target(geometry, node_rect, drop_point))
        .max_by(|left, right| {
            left.depth
                .cmp(&right.depth)
                .then_with(|| left.center_inside.cmp(&right.center_inside))
                .then_with(|| left.pointer_inside.cmp(&right.pointer_inside))
                .then_with(|| left.overlap_ratio.total_cmp(&right.overlap_ratio))
                .then_with(|| left.stacking_order.cmp(&right.stacking_order))
        });
    if selected.is_some() {
        return selected;
    }

    // The Composition is the root graph owner. A deliberate drop beyond its
    // current bounds remains legal; the atomic reparent step expands the root
    // on all four sides so visual containment catches up without moving the
    // Node. Nested containers never receive this fallback.
    let root = geometries
        .into_iter()
        .find(|geometry| geometry.container == NodeContainer::Composition(composition_id))?;
    let content_rect = root.content_rect?;
    Some(ReparentTargetEvaluation {
        container: root.container,
        visible_rect: root.visible_rect,
        content_rect,
        depth: root.depth,
        stacking_order: root.stacking_order,
        overlap_ratio: overlap_ratio(content_rect, node_rect),
        center_inside: false,
        pointer_inside: false,
        root_fallback: true,
        score: -1.0,
        kind: ReparentTargetKind::RootFallback,
    })
}

pub(super) fn node_drop_intents(
    project: &Project,
    composition_id: Uuid,
    gesture: &NodeEditorReparentGesture,
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    final_positions: &HashMap<Uuid, [f32; 2]>,
    drop_point: egui::Pos2,
    canvas_scale: f32,
) -> Vec<NodeDropIntent> {
    let scale = sanitized_node_editor_scale(canvas_scale);
    let mut node_ids = gesture.origins.keys().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();
    let final_rects = final_node_rects(project, gesture, rendered_node_rects, final_positions);
    let Some(primary_node_id) = gesture.primary_node_id else {
        return Vec::new();
    };
    let Some(primary_origin) = gesture.origins.get(&primary_node_id) else {
        return Vec::new();
    };
    let Some(primary_final_position) =
        final_positions.get(&primary_node_id).copied().or_else(|| {
            project
                .get_node(primary_node_id)
                .map(|node| node.ui_position)
        })
    else {
        return Vec::new();
    };
    let displacement = egui::vec2(
        primary_final_position[0] - primary_origin.position[0],
        primary_final_position[1] - primary_origin.position[1],
    );
    let Some(primary_rect) = final_rects.get(&primary_node_id).copied() else {
        return Vec::new();
    };
    if displacement.length() * scale < NODE_REPARENT_DRAG_THRESHOLD {
        return retained_origin_intents(project, composition_id, gesture, &final_rects, drop_point);
    }
    let Some(resolved_target) =
        deepest_legal_reparent_target(project, composition_id, primary_rect, drop_point)
    else {
        return Vec::new();
    };
    if resolved_target.container == primary_origin.container {
        if !resolved_target.root_fallback {
            return retained_origin_intents(
                project,
                composition_id,
                gesture,
                &final_rects,
                drop_point,
            );
        }
    } else if !node_has_clearly_exited_origin(
        project,
        composition_id,
        primary_origin.container,
        resolved_target.container,
        primary_rect,
        drop_point,
    ) {
        // Header/padding hysteresis retains canonical ownership, but the Node
        // still has an exact final position. Grow the old owner around that
        // position atomically so a later auto-layout pass cannot move it.
        return retained_origin_intents(project, composition_id, gesture, &final_rects, drop_point);
    }

    // A multi-selected drag is one semantic move. Resolve ownership from the
    // physically grabbed Node, then keep every selected Node's relative
    // layout by applying that same target in one candidate transaction.
    node_ids
        .into_iter()
        .filter_map(|node_id| {
            final_rects
                .get(&node_id)
                .copied()
                .map(|final_rect| NodeDropIntent {
                    node_id,
                    final_rect,
                    target: resolved_target,
                })
        })
        .collect()
}

fn final_node_rects(
    project: &Project,
    gesture: &NodeEditorReparentGesture,
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    final_positions: &HashMap<Uuid, [f32; 2]>,
) -> HashMap<Uuid, egui::Rect> {
    gesture
        .origins
        .keys()
        .filter_map(|node_id| {
            let rect = rendered_node_rects.get(node_id).copied()?;
            let current = project.get_node(*node_id)?.ui_position;
            let final_position = final_positions.get(node_id).copied().unwrap_or(current);
            let frame_offset = rect.min - egui::pos2(current[0], current[1]);
            Some((
                *node_id,
                egui::Rect::from_min_size(
                    egui::pos2(final_position[0], final_position[1]) + frame_offset,
                    rect.size(),
                ),
            ))
        })
        .collect()
}

pub(super) fn retained_origin_intents(
    project: &Project,
    composition_id: Uuid,
    gesture: &NodeEditorReparentGesture,
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    drop_point: egui::Pos2,
) -> Vec<NodeDropIntent> {
    let mut node_ids = gesture.origins.keys().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();
    node_ids
        .into_iter()
        .filter_map(|node_id| {
            let origin = gesture.origins.get(&node_id)?.container;
            let final_rect = rendered_node_rects.get(&node_id).copied()?;
            let target = retained_origin_reparent_target(
                project,
                composition_id,
                origin,
                final_rect,
                drop_point,
            )?;
            (!rect_contains_rect(target.content_rect, final_rect)).then_some(NodeDropIntent {
                node_id,
                final_rect,
                target,
            })
        })
        .collect()
}

pub(super) fn retained_origin_reparent_target(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    node_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> Option<ReparentTargetEvaluation> {
    let geometry = reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == origin)?;
    let content_rect = geometry.content_rect.unwrap_or(geometry.visible_rect);
    Some(ReparentTargetEvaluation {
        container: origin,
        visible_rect: geometry.visible_rect,
        content_rect,
        depth: geometry.depth,
        stacking_order: geometry.stacking_order,
        overlap_ratio: overlap_ratio(content_rect, node_rect),
        center_inside: content_rect.contains(node_rect.center()),
        pointer_inside: content_rect.contains(drop_point),
        root_fallback: false,
        score: -0.5,
        kind: ReparentTargetKind::RetainedOrigin,
    })
}

pub(super) fn target_is_deeper_within_origin(
    project: &Project,
    origin: NodeContainer,
    target: NodeContainer,
) -> bool {
    match (origin, target) {
        (NodeContainer::Composition(composition_id), NodeContainer::Track(track_id)) => {
            project.find_composition_for_track(track_id) == Some(composition_id)
        }
        (NodeContainer::Composition(composition_id), NodeContainer::Clip(clip_id)) => {
            project
                .find_track_for_clip(clip_id)
                .and_then(|track_id| project.find_composition_for_track(track_id))
                == Some(composition_id)
        }
        (NodeContainer::Track(track_id), NodeContainer::Clip(clip_id)) => {
            project.find_track_for_clip(clip_id) == Some(track_id)
        }
        _ => false,
    }
}

pub(super) fn target_is_authoritative_sibling(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    target: NodeContainer,
    drop_point: egui::Pos2,
) -> bool {
    let origin_parent = parent_container_owner(project, port_owner_for_node_container(origin));
    let target_parent = parent_container_owner(project, port_owner_for_node_container(target));
    if origin == target || origin_parent.is_none() || origin_parent != target_parent {
        return false;
    }
    reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == target)
        .is_some_and(|geometry| {
            geometry
                .content_rect
                .unwrap_or(geometry.visible_rect)
                .contains(drop_point)
        })
}

pub(super) fn node_has_clearly_exited_origin(
    project: &Project,
    composition_id: Uuid,
    origin: NodeContainer,
    target: NodeContainer,
    final_rect: egui::Rect,
    drop_point: egui::Pos2,
) -> bool {
    if target_is_deeper_within_origin(project, origin, target)
        || target_is_authoritative_sibling(project, composition_id, origin, target, drop_point)
    {
        return true;
    }
    let origin_geometry = reparent_container_geometries(project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == origin);
    origin_geometry.is_some_and(|geometry| {
        !geometry.visible_rect.intersects(final_rect) && !geometry.visible_rect.contains(drop_point)
    })
}

pub(super) fn record_node_reparent_origins(
    project: &Project,
    layout_edits: &[LayoutEdit],
    grabbed_node: Option<Uuid>,
    state: &mut NodeEditorState,
) {
    for edit in layout_edits {
        let LayoutEdit::MoveNode { node_id, .. } = edit else {
            continue;
        };
        let Some(node) = project.get_node(*node_id) else {
            continue;
        };
        let Some(container) = project.find_node_container(*node_id) else {
            continue;
        };
        let gesture = state
            .node_reparent
            .get_or_insert_with(|| NodeEditorReparentGesture {
                origins: HashMap::new(),
                primary_node_id: None,
                hovered_target: None,
                hovered_node_id: None,
                hovered_score: None,
            });
        gesture
            .origins
            .entry(*node_id)
            .or_insert(NodeEditorNodeDragOrigin {
                container,
                position: node.ui_position,
            });
    }
    if let (Some(grabbed_node), Some(gesture)) = (grabbed_node, state.node_reparent.as_mut()) {
        if gesture.origins.contains_key(&grabbed_node) {
            gesture.primary_node_id = Some(grabbed_node);
        }
    }
}

pub(super) fn final_node_positions(
    project: &Project,
    gesture: &NodeEditorReparentGesture,
    layout_edits: &[LayoutEdit],
) -> HashMap<Uuid, [f32; 2]> {
    let mut positions = gesture
        .origins
        .keys()
        .filter_map(|node_id| {
            project
                .get_node(*node_id)
                .map(|node| (*node_id, node.ui_position))
        })
        .collect::<HashMap<_, _>>();
    for edit in layout_edits {
        if let LayoutEdit::MoveNode { node_id, position } = edit {
            if positions.contains_key(node_id) {
                positions.insert(*node_id, *position);
            }
        }
    }
    positions
}

pub(super) fn primary_node_drop_intent(
    intents: &[NodeDropIntent],
    drop_point: egui::Pos2,
) -> Option<NodeDropIntent> {
    intents
        .iter()
        .copied()
        .find(|intent| intent.final_rect.contains(drop_point))
        .or_else(|| intents.first().copied())
}

pub(super) fn reparent_ineligible_reason(
    geometry: ReparentContainerGeometry,
) -> Option<&'static str> {
    geometry.collapsed.then_some("collapsed_hidden_content")
}

pub(super) fn reported_reparent_target(
    evaluation: Option<ReparentTargetEvaluation>,
    selected: bool,
    active: NodeDropIntent,
) -> Option<ReparentTargetEvaluation> {
    if selected {
        Some(active.target)
    } else {
        evaluation
    }
}

pub(super) fn register_reparent_drop_targets(
    project: &Project,
    composition_id: Uuid,
    active: NodeDropIntent,
    drop_point: egui::Pos2,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    painter: &egui::Painter,
) {
    for geometry in reparent_container_geometries(project, composition_id) {
        let evaluation = evaluate_reparent_target(geometry, active.final_rect, drop_point);
        let selected = active.target.container == geometry.container;
        let reported = reported_reparent_target(evaluation, selected, active);
        let graph_rect = geometry.content_rect.unwrap_or(geometry.visible_rect);
        let unclipped_rect = to_global * graph_rect;
        let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
        let owner = qa_container_key(port_owner_for_node_container(geometry.container));
        let (overlap, center_inside, pointer_inside, score) =
            reported.map_or((0.0, false, false, None), |evaluation| {
                (
                    evaluation.overlap_ratio,
                    evaluation.center_inside,
                    evaluation.pointer_inside,
                    Some(evaluation.score),
                )
            });
        crate::qa::register_component_with_metadata(
            format!("node_editor.reparent_target.{owner}"),
            "node_reparent_target",
            rect,
            reported.is_some(),
            Some(serde_json::json!({
                "owner": owner,
                "active_node_id": active.node_id,
                "selected": selected,
                "eligible": reported.is_some(),
                "geometry_eligible": evaluation.is_some(),
                "collapsed": geometry.collapsed,
                "ineligible_reason": reparent_ineligible_reason(geometry),
                "selection_kind": selected.then_some(active.target.kind.as_str()),
                "depth": geometry.depth,
                "stacking_order": geometry.stacking_order,
                "overlap_ratio": overlap,
                "center_inside": center_inside,
                "pointer_inside": pointer_inside,
                "root_fallback": selected && active.target.root_fallback,
                "score": score,
                "drop_point": {"x": drop_point.x, "y": drop_point.y},
                "node_rect": qa_rect_metadata(active.final_rect),
                "content_rect": geometry.content_rect.map(qa_rect_metadata),
                "drop_target_rect": qa_rect_metadata(graph_rect),
                "unclipped_rect": qa_rect_metadata(unclipped_rect),
                "visible_in_canvas": rect.is_positive(),
            })),
        );
    }

    let highlight = if active.target.root_fallback {
        active.target.visible_rect
    } else {
        active.target.content_rect
    };
    painter.rect_filled(
        highlight,
        egui::CornerRadius::same(8),
        Color32::from_rgba_premultiplied(78, 190, 128, 22),
    );
    painter.rect_stroke(
        highlight,
        egui::CornerRadius::same(8),
        egui::Stroke::new(
            screen_stroke_in_graph_units(2.0, to_global.scaling),
            Color32::from_rgb(94, 221, 151),
        ),
        egui::StrokeKind::Inside,
    );
}

pub(super) fn reparent_nodes_from_intents(
    project: &mut Project,
    intents: &[NodeDropIntent],
) -> bool {
    let mut candidate = project.clone();
    let mut changed = false;
    for intent in intents {
        let node_id = intent.node_id;
        let destination = intent.target.container;
        let containment_rect = estimated_node_rect(&candidate, node_id)
            .map_or(intent.final_rect, |estimated| {
                estimated.union(intent.final_rect)
            });
        if candidate.find_node_container(node_id) == Some(destination) {
            changed |=
                ensure_reparent_hierarchy_contains(&mut candidate, destination, containment_rect);
            continue;
        }
        match candidate.attach_node_to_container(destination, node_id) {
            Ok(()) => {
                ensure_reparent_hierarchy_contains(&mut candidate, destination, containment_rect);
                changed = true;
            }
            Err(error) => {
                log::warn!("Cannot move Node {node_id} to {destination:?}: {error}");
                return false;
            }
        }
    }
    if changed {
        *project = candidate;
    }
    changed
}

pub(super) fn finish_node_reparent(
    project: &mut Project,
    intents: &[NodeDropIntent],
    gesture: Option<&NodeEditorReparentGesture>,
) -> ReparentReleaseOutcome {
    if intents.is_empty() {
        return ReparentReleaseOutcome::NoIntent;
    }
    let mut baseline = project.clone();
    restore_reparent_origins(&mut baseline, gesture);
    let mut candidate = project.clone();
    if reparent_nodes_from_intents(&mut candidate, intents) {
        let composition_ids = intents
            .iter()
            .filter_map(|intent| {
                port_owner_composition(
                    &candidate,
                    port_owner_for_node_container(intent.target.container),
                )
            })
            .collect::<HashSet<_>>();
        let introduces_invalid_layout = composition_ids.into_iter().any(|composition_id| {
            !container_hierarchy_needs_reflow(&baseline, composition_id)
                && container_hierarchy_needs_reflow(&candidate, composition_id)
        });
        if !introduces_invalid_layout {
            *project = candidate;
            return ReparentReleaseOutcome::Applied;
        }
        log::warn!("Rejected Node reparent because it would require automatic layout repair");
    }

    // The live graph follows the pointer during the drag. If the semantic
    // candidate is rejected, roll every participating Node back together so
    // a half-applied layout cannot remain under the old owner or splice a wire.
    restore_reparent_origins(project, gesture);
    ReparentReleaseOutcome::Rejected
}

pub(super) fn restore_reparent_origins(
    project: &mut Project,
    gesture: Option<&NodeEditorReparentGesture>,
) {
    if let Some(gesture) = gesture {
        for (node_id, origin) in &gesture.origins {
            if let Some(node) = project.get_node_mut(*node_id) {
                node.ui_position = origin.position;
            }
        }
    }
}

pub(super) fn splice_node_for_release(
    outcome: ReparentReleaseOutcome,
    moved_node_ids: &HashSet<Uuid>,
    captured_primary_node_id: Option<Uuid>,
) -> Option<Uuid> {
    if outcome != ReparentReleaseOutcome::NoIntent || moved_node_ids.len() != 1 {
        return None;
    }
    let node_id = moved_node_ids.iter().next().copied()?;
    (captured_primary_node_id == Some(node_id)).then_some(node_id)
}

#[cfg(test)]
pub(super) fn reparent_test_node_at_drop(
    project: &mut Project,
    composition_id: Uuid,
    node_id: Uuid,
    drop_point: egui::Pos2,
) -> bool {
    let node_size = estimated_node_size(project, node_id);
    let final_rect = egui::Rect::from_center_size(drop_point, node_size);
    let Some(node) = project.get_node_mut(node_id) else {
        return false;
    };
    node.ui_position = [final_rect.min.x, final_rect.min.y];
    let Some(target) =
        deepest_legal_reparent_target(project, composition_id, final_rect, drop_point)
    else {
        return false;
    };
    reparent_nodes_from_intents(
        project,
        &[NodeDropIntent {
            node_id,
            final_rect,
            target,
        }],
    )
}

pub(super) fn container_rect(position: [f32; 2], size: [f32; 2]) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(position[0], position[1]),
        egui::vec2(
            size[0].max(MIN_CONTAINER_SIZE.x),
            size[1].max(MIN_CONTAINER_SIZE.y),
        ),
    )
}

pub(super) fn ensure_container_hierarchy_contains(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
) -> bool {
    let owner = port_owner_for_node_container(container);
    let mut changed = grow_container_to_rect(project, owner, node_rect);
    changed |= ensure_structural_merge_layout(project, container);

    // Creation and auto-layout clamp their leaf Node to the owning container's
    // minimum content edge. Preserve existing container origins here and grow
    // only the far edges, otherwise adding an item can move an entire existing
    // hierarchy merely because legacy child chrome sits inside its margin.
    let parent_track = match container {
        NodeContainer::Clip(clip_id) => project.find_track_for_clip(clip_id),
        NodeContainer::Track(track_id) => Some(track_id),
        NodeContainer::Composition(_) => None,
    };
    if let Some(track_id) = parent_track {
        if let NodeContainer::Clip(clip_id) = container {
            if let Some(clip) = project.get_clip(clip_id) {
                let clip_rect = container_rect(clip.ui_position, clip.ui_size);
                changed |= grow_container_to_rect(project, PortOwner::Track(track_id), clip_rect);
            }
            changed |= ensure_structural_merge_layout(project, NodeContainer::Track(track_id));
        }
        if let Some(composition_id) = project.find_composition_for_track(track_id) {
            if let Some(track) = project.get_track(track_id) {
                let track_rect = container_rect(track.ui_position, track.ui_size);
                changed |= grow_container_to_rect(
                    project,
                    PortOwner::Composition(composition_id),
                    track_rect,
                );
            }
            changed |=
                ensure_structural_merge_layout(project, NodeContainer::Composition(composition_id));
        }
    }
    changed
}

pub(super) fn ensure_reparent_hierarchy_contains(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
) -> bool {
    grow_reparent_hierarchy(project, container, node_rect, true)
}

/// Grow only the direct owning container and its ancestors around `node_rect`.
///
/// Directional layout already owns the exact final position of every Node it
/// may change. In particular, it must not run structural Merge repair during
/// commit because that can move Nodes which were fixed in the preview.
pub(super) fn grow_container_hierarchy_to_rect_all_edges(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
) -> bool {
    grow_reparent_hierarchy(project, container, node_rect, false)
}

fn grow_reparent_hierarchy(
    project: &mut Project,
    container: NodeContainer,
    node_rect: egui::Rect,
    repair_structural_merges: bool,
) -> bool {
    let owner = port_owner_for_node_container(container);
    let mut changed = grow_container_to_rect_all_edges(project, owner, node_rect);
    if repair_structural_merges {
        changed |= ensure_structural_merge_layout(project, container);
    }

    // Propagate each *updated child container rectangle*, not only the Node.
    // Expanding the min edge intentionally changes only container chrome;
    // child Nodes keep their absolute graph coordinates and therefore their
    // exact drop position.
    let mut child_owner = owner;
    while let Some(parent_owner) = parent_container_owner(project, child_owner) {
        let Some(child) = container_visual(project, child_owner) else {
            break;
        };
        let child_rect = container_rect(child.position, child.size);
        changed |= grow_container_to_rect_all_edges(project, parent_owner, child_rect);
        if repair_structural_merges {
            if let Some(parent_container) = node_container_for_port_owner(parent_owner) {
                changed |= ensure_structural_merge_layout(project, parent_container);
            }
        }
        child_owner = parent_owner;
    }
    changed
}

pub(super) fn node_container_for_port_owner(owner: PortOwner) -> Option<NodeContainer> {
    match owner {
        PortOwner::Composition(id) => Some(NodeContainer::Composition(id)),
        PortOwner::Track(id) => Some(NodeContainer::Track(id)),
        PortOwner::Clip(id) => Some(NodeContainer::Clip(id)),
        PortOwner::Node(_) => None,
    }
}

pub(super) fn grow_container_to_rect(
    project: &mut Project,
    owner: PortOwner,
    rect: egui::Rect,
) -> bool {
    let Some(visual) = container_visual(project, owner) else {
        return false;
    };
    let (right_margin, bottom_margin) = match owner {
        PortOwner::Composition(_) => (
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ),
        PortOwner::Track(_) | PortOwner::Clip(_) => {
            (AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_BOTTOM)
        }
        PortOwner::Node(_) => return false,
    };
    let size = [
        visual.size[0].max(rect.right() - visual.position[0] + right_margin),
        visual.size[1].max(rect.bottom() - visual.position[1] + bottom_margin),
    ];
    set_container_size(project, owner, size)
}

pub(super) fn grow_container_to_rect_all_edges(
    project: &mut Project,
    owner: PortOwner,
    rect: egui::Rect,
) -> bool {
    let Some(visual) = container_visual(project, owner) else {
        return false;
    };
    let (left_margin, top_margin, right_margin, bottom_margin) = match owner {
        PortOwner::Composition(_) => (
            AUTO_LAYOUT_COMPOSITION_LEFT,
            AUTO_LAYOUT_COMPOSITION_TOP,
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ),
        PortOwner::Track(_) => (
            AUTO_LAYOUT_TRACK_LEFT,
            AUTO_LAYOUT_TRACK_TOP,
            AUTO_LAYOUT_TRACK_RIGHT,
            AUTO_LAYOUT_TRACK_BOTTOM,
        ),
        PortOwner::Clip(_) => (
            AUTO_LAYOUT_TRACK_LEFT,
            AUTO_LAYOUT_CLIP_TOP,
            AUTO_LAYOUT_TRACK_RIGHT,
            AUTO_LAYOUT_TRACK_BOTTOM,
        ),
        PortOwner::Node(_) => return false,
    };
    let old_rect = container_rect(visual.position, visual.size);
    let left = old_rect.left().min(rect.left() - left_margin);
    let top = old_rect.top().min(rect.top() - top_margin);
    let right = old_rect.right().max(rect.right() + right_margin);
    let bottom = old_rect.bottom().max(rect.bottom() + bottom_margin);
    let position = [left, top];
    let size = [
        (right - left).max(MIN_CONTAINER_SIZE.x),
        (bottom - top).max(MIN_CONTAINER_SIZE.y),
    ];
    set_container_geometry(project, owner, position, size)
}
