use super::*;

type CreateAction = Box<dyn FnOnce(&mut Project) -> bool>;
pub(super) fn create_action_for_request(
    request: NodeCreateRequest,
    project_service: &EditorService,
    canvas_size: (u64, u64),
    graph_position: egui::Pos2,
    comp_id: Uuid,
) -> Option<CreateAction> {
    let plugin_manager = project_service.get_plugin_manager();
    match request {
        NodeCreateRequest::Native(catalog_id) => create_native_action(
            &catalog_id,
            project_service,
            canvas_size,
            graph_position,
            comp_id,
        ),
        transform_request @ (NodeCreateRequest::ShapeTransform
        | NodeCreateRequest::ImageTransform) => {
            let node =
                create_operation_node_for_request(&transform_request, plugin_manager.as_ref())?;
            Some(Box::new(move |project| {
                insert_prebuilt_graph(
                    project,
                    graph_position,
                    NodeGraphBundle::new(vec![node], Vec::new(), None),
                    comp_id,
                )
            }))
        }
        NodeCreateRequest::Style(component_id) => {
            match plugin_manager.create_style_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Style Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Effector(component_id) => {
            match plugin_manager.create_effector_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Effector Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::PathEffect(component_id) => {
            match plugin_manager.create_path_effect_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Path Effect Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Decorator(component_id) => {
            match plugin_manager.create_decorator_operation_node(&component_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Decorator Node {component_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::Effect(effect_id) => {
            match plugin_manager.create_effect_operation_node(&effect_id) {
                Ok(node) => Some(Box::new(move |project| {
                    insert_prebuilt_graph(
                        project,
                        graph_position,
                        NodeGraphBundle::new(vec![node], Vec::new(), None),
                        comp_id,
                    )
                })),
                Err(error) => {
                    log::error!("Cannot create Effect Node {effect_id}: {error}");
                    None
                }
            }
        }
        NodeCreateRequest::SoundMerge => Some(Box::new(move |project| {
            create_prebuilt_node(
                project,
                graph_position,
                Node::new_sound_merge("Sound Merge"),
                comp_id,
            )
        })),
        NodeCreateRequest::SoundAnalysis(analysis) => Some(Box::new(move |project| {
            create_prebuilt_node(
                project,
                graph_position,
                Node::new_sound_analysis(analysis.label(), analysis),
                comp_id,
            )
        })),
        NodeCreateRequest::Clip => Some(Box::new(move |project| {
            create_clip_at_free_slot(project, graph_position, comp_id, "Clip").is_some()
        })),
        NodeCreateRequest::Track => Some(Box::new(move |project| {
            create_track_at_free_slot(project, graph_position, comp_id, "Track").is_some()
        })),
        NodeCreateRequest::Composition => Some(Box::new(move |project| {
            create_composition_node(project, graph_position, comp_id)
        })),
    }
}

fn create_native_action(
    catalog_id: &str,
    project_service: &EditorService,
    canvas_size: (u64, u64),
    graph_position: egui::Pos2,
    comp_id: Uuid,
) -> Option<CreateAction> {
    use library::model::{native_node_descriptor, GeneratorContent, NativeNodeFactory};

    let descriptor = native_node_descriptor(catalog_id)?;
    let result = match descriptor.factory() {
        NativeNodeFactory::Generator(GeneratorContent::Text) => project_service.create_text_node(
            "Hello World",
            library::editor::project_service::DEFAULT_TEXT_FONT,
            canvas_size.0,
            canvas_size.1,
        ),
        NativeNodeFactory::Generator(GeneratorContent::Solid) => project_service.create_solid_node(
            library::model::frame::color::Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
            canvas_size.0,
            canvas_size.1,
        ),
        NativeNodeFactory::Generator(GeneratorContent::Shape) => project_service.create_shape_node(
            library::editor::project_service::DEFAULT_SHAPE_PATH,
            canvas_size.0,
            canvas_size.1,
            100,
            100,
        ),
        NativeNodeFactory::Generator(GeneratorContent::SkSL) => project_service.create_sksl_node(
            library::editor::project_service::DEFAULT_SKSL_SHADER,
            canvas_size.0,
            canvas_size.1,
        ),
        NativeNodeFactory::Value(_)
        | NativeNodeFactory::Merge
        | NativeNodeFactory::TypedPlaceholder => descriptor
            .create_detached_node()
            .map_err(library::error::LibraryError::Validation),
    };
    match result {
        Ok(mut node) => {
            node.name = descriptor.label().to_string();
            Some(Box::new(move |project| {
                create_prebuilt_node(project, graph_position, node, comp_id)
            }))
        }
        Err(error) => {
            log::error!(
                "Cannot create native catalog Node '{}': {error}",
                descriptor.catalog_id()
            );
            None
        }
    }
}

pub(super) struct NodeContextMenuFrame<'a> {
    pub(super) project_lock: &'a Arc<RwLock<Project>>,
    pub(super) project_service: &'a EditorService,
    pub(super) comp_id: Uuid,
    pub(super) exclusion_rects: &'a [egui::Rect],
    pub(super) to_global: egui::emath::TSTransform,
    pub(super) suppress_secondary_click: bool,
}

pub(super) fn handle_context_menu(
    ui: &mut egui::Ui,
    state: &mut Option<ContextMenuState>,
    frame: NodeContextMenuFrame<'_>,
) -> bool {
    let canvas_size = frame
        .project_lock
        .read()
        .ok()
        .and_then(|project| {
            project
                .get_composition(frame.comp_id)
                .map(|composition| (composition.width, composition.height))
        })
        .unwrap_or((1920, 1080));
    let from_global = frame.to_global.inverse();
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.interact_pos(),
            input.time,
        )
    });
    update_global_context_menu_for_secondary_click(
        state,
        secondary_clicked && !frame.suppress_secondary_click,
        pointer_position,
        ui.min_rect(),
        frame.exclusion_rects,
        frame.to_global,
        open_time,
    );
    let mut should_close = false;
    let mut action: Option<CreateAction> = None;
    if let Some(context) = state {
        let position = context.position;
        let graph_position = from_global * position;
        let popup =
            searchable_popup_placement(position, egui::vec2(320.0, 348.0), ui.ctx().content_rect());
        let menu_id = format!("node_editor_add_menu:{}", context.open_time.to_bits());
        let response = egui::Area::new(egui::Id::new("node_ctx_menu"))
            .order(egui::Order::Foreground)
            .pivot(popup.pivot)
            .fixed_pos(popup.area_anchor)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                show_searchable_popup_frame(ui, popup, |ui| {
                    let plugin_manager = frame.project_service.get_plugin_manager();
                    let items = node_create_menu_items(plugin_manager.as_ref());
                    if let Some(request) = show_searchable_items_with_qa(
                        ui,
                        &menu_id,
                        Some("node_editor.menu.search"),
                        &items,
                    ) {
                        action = create_action_for_request(
                            request,
                            frame.project_service,
                            canvas_size,
                            graph_position,
                            frame.comp_id,
                        );
                        should_close = true;
                    }
                })
            });
        let root_rect = response.inner.response.rect;
        register_searchable_popup_qa("node_editor.menu.root", position, popup, root_rect);
        if ui.input(|input| input.pointer.any_click())
            && ui.input(|input| input.time) - context.open_time > 0.2
            && searchable_menu_click_is_outside(ui.ctx(), &menu_id, root_rect)
        {
            should_close = true;
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
    }
    let mut changed = false;
    if let Some(action) = action {
        if let Ok(mut project) = frame.project_lock.write() {
            changed = action(&mut project);
        }
    }
    if should_close {
        *state = None;
    }
    changed
}

pub(super) fn update_global_context_menu_for_secondary_click(
    state: &mut Option<ContextMenuState>,
    secondary_clicked: bool,
    pointer_position: Option<egui::Pos2>,
    canvas_rect: egui::Rect,
    exclusion_rects: &[egui::Rect],
    to_global: egui::emath::TSTransform,
    open_time: f64,
) {
    if !secondary_clicked {
        return;
    }
    let Some(position) = pointer_position.filter(|position| canvas_rect.contains(*position)) else {
        return;
    };
    let graph_position = to_global.inverse() * position;
    if exclusion_rects
        .iter()
        .any(|rect| rect.contains(graph_position))
    {
        // A Snarl item owns this gesture. Also close a stale Create menu so a
        // Node/container menu and the global menu cannot remain visible at the
        // same time after a secondary click.
        *state = None;
        return;
    }
    *state = Some(ContextMenuState::new(position, open_time));
}

pub(super) fn push_history_snapshot(
    project_lock: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) {
    if let Ok(project) = project_lock.read() {
        history_manager.push_project_state(project.clone());
    }
}

#[cfg(test)]
pub(super) fn available_effector_menu_entries(
    plugin_manager: &PluginManager,
) -> Vec<(String, String)> {
    let mut entries = plugin_manager
        .get_available_effectors()
        .into_iter()
        .filter_map(|component_id| {
            match plugin_manager.operation_descriptor(
                EFFECTOR_CATEGORY,
                &component_id,
                EFFECTOR_APPLY_OPERATION,
            ) {
                Ok(descriptor) => Some((component_id, descriptor.label().to_string())),
                Err(error) => {
                    log::warn!("Cannot expose Effector {component_id} in the Node Editor: {error}");
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    entries
}

pub(super) fn insert_prebuilt_graph(
    project: &mut Project,
    desired: egui::Pos2,
    mut graph: NodeGraphBundle,
    composition_id: Uuid,
) -> bool {
    let Some(container) = node_container_at_position(project, composition_id, desired) else {
        return false;
    };
    let Some((container_position, container_size, existing_node_ids)) =
        container_geometry(project, container)
    else {
        return false;
    };

    // Factory coordinates are only hints. Plugin-backed Nodes may have many
    // more property ports than a factory can anticipate, so lay the detached
    // graph out from its canonical connections and the same conservative
    // card measurements used by the rest of this editor before placement.
    // This is deliberately an app concern: the library factory remains a
    // renderer/UI-independent graph constructor.
    layout_detached_node_graph(project, &mut graph);

    // `output_node_id` identifies the consumer/sink within a detached factory
    // graph. It is useful for creating a brand-new Clip, but ordinary Add in
    // an existing container must never silently replace that container's
    // explicit output binding. Setting an output remains a separate command.
    graph.output_node_id = None;

    // Measure with the same canonical port-derived estimator used for
    // existing Nodes. A temporary Project keeps this layout calculation out
    // of the authoritative model until the atomic insert succeeds.
    let mut measurement_project = project.clone();
    for node in &graph.nodes {
        measurement_project.add_node(node.clone());
    }
    let mut graph_bounds = egui::Rect::NOTHING;
    for node in &graph.nodes {
        let rect = egui::Rect::from_min_size(
            egui::pos2(node.ui_position[0], node.ui_position[1]),
            estimated_node_size(&measurement_project, node.id),
        );
        graph_bounds = graph_bounds.union(rect);
    }
    if !graph_bounds.is_finite() || !graph_bounds.is_positive() {
        return false;
    }

    let content_left = container_position[0]
        + match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_LEFT,
            NodeContainer::Track(_) | NodeContainer::Clip(_) => AUTO_LAYOUT_TRACK_LEFT,
        };
    let content_top = container_position[1]
        + match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
            NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
            NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
        };
    let max_x = (container_position[0] + container_size[0]
        - AUTO_LAYOUT_TRACK_RIGHT
        - graph_bounds.width())
    .max(content_left);
    let anchor = egui::pos2(
        desired.x.max(content_left).clamp(content_left, max_x),
        desired.y.max(content_top),
    );
    let mut candidate = egui::Rect::from_min_size(anchor, graph_bounds.size());
    let mut occupied = existing_node_ids
        .iter()
        .filter_map(|node_id| estimated_node_rect(project, *node_id))
        .collect::<Vec<_>>();
    occupied.extend(immediate_child_rects(
        project,
        &AutoLayoutPlan::default(),
        container,
    ));
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| rects_are_closer_than(candidate, **other, DETACHED_GRAPH_NODE_GAP))
            .map(|other| other.bottom() + DETACHED_GRAPH_NODE_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(anchor.x, next_y), graph_bounds.size());
    }

    let translation = candidate.min - graph_bounds.min;
    for node in &mut graph.nodes {
        node.ui_position[0] += translation.x;
        node.ui_position[1] += translation.y;
    }
    if let Err(error) = project.insert_node_graph(container, graph) {
        log::warn!("Cannot insert Node graph into {container:?}: {error}");
        return false;
    }

    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_collapsed = false;
    }
    ensure_container_hierarchy_contains(project, container, candidate);
    true
}

pub(super) fn layout_detached_node_graph(project: &Project, graph: &mut NodeGraphBundle) {
    if graph.nodes.is_empty() {
        return;
    }

    let node_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let node_set = node_ids.iter().copied().collect::<HashSet<_>>();
    let mut edges = graph
        .connections
        .iter()
        .filter_map(|connection| {
            let (PortOwner::Node(from), PortOwner::Node(to)) =
                (connection.from.owner, connection.to.owner)
            else {
                return None;
            };
            (node_set.contains(&from) && node_set.contains(&to)).then_some((from, to))
        })
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    let ranks = rank_nodes_by_scc(&node_ids, &edges);

    let mut measurement_project = project.clone();
    for node in &graph.nodes {
        measurement_project.add_node(node.clone());
    }

    // Preserve factory order within a rank. Connection order has semantic
    // meaning for multi-input Style ports, and matching that order visually
    // makes Fill-before-Stroke immediately legible.
    let mut columns = BTreeMap::<usize, Vec<Uuid>>::new();
    for node_id in &node_ids {
        columns
            .entry(ranks.get(node_id).copied().unwrap_or_default())
            .or_default()
            .push(*node_id);
    }

    // Half-open geometry would allow an exact 24 px gap. egui Rect
    // intersection treats touching expanded edges as an intersection, so a
    // tiny extra margin keeps this layout out of the reflow detector while
    // preserving the intended 24 px visual rhythm.
    let gap = DETACHED_GRAPH_NODE_GAP;
    let mut column_x = BTreeMap::<usize, f32>::new();
    let mut x = 0.0;
    for (rank, node_ids) in &columns {
        column_x.insert(*rank, x);
        let width = node_ids
            .iter()
            .map(|node_id| estimated_node_size(&measurement_project, *node_id).x)
            .max_by(f32::total_cmp)
            .unwrap_or_default();
        x += width + gap;
    }

    let mut positions = HashMap::<Uuid, [f32; 2]>::new();
    for (rank, node_ids) in columns {
        let mut y = 0.0;
        for node_id in node_ids {
            positions.insert(node_id, [column_x[&rank], y]);
            y += estimated_node_size(&measurement_project, node_id).y + gap;
        }
    }
    for node in &mut graph.nodes {
        if let Some(position) = positions.get(&node.id) {
            node.ui_position = *position;
        }
    }
}

pub(super) fn create_prebuilt_node(
    project: &mut Project,
    position: egui::Pos2,
    mut node: Node,
    comp_id: Uuid,
) -> bool {
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    project.add_node(node);
    if let Some(container) = attach_node_at_position(project, node_id, comp_id, position) {
        place_node_in_free_slot(project, node_id, container, position, &[]);
        true
    } else {
        if let Err(error) = project.remove_node(node_id) {
            log::warn!("Cannot roll back unattached Node {node_id}: {error}");
        }
        false
    }
}

pub(super) fn create_composition_node(
    project: &mut Project,
    position: egui::Pos2,
    comp_id: Uuid,
) -> bool {
    let mut candidate = project.clone();
    let (composition, root) =
        library::model::Composition::new("Nested Comp", 1920, 1080, 30.0, 10.0);
    let nested_id = composition.id;
    if candidate
        .add_track(root)
        .and_then(|()| candidate.add_composition(composition))
        .is_err()
    {
        return false;
    }

    let mut node = Node::new_composition_instance(
        "Container",
        library::model::CompositionInstanceContent {
            composition_id: nested_id,
        },
    );
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    candidate.add_node(node);
    if let Some(container) = attach_node_at_position(&mut candidate, node_id, comp_id, position) {
        place_node_in_free_slot(&mut candidate, node_id, container, position, &[]);
        *project = candidate;
        true
    } else {
        false
    }
}

pub(super) fn attach_node_at_position(
    project: &mut Project,
    node_id: Uuid,
    comp_id: Uuid,
    position: egui::Pos2,
) -> Option<NodeContainer> {
    let container = node_container_at_position(project, comp_id, position)?;
    if let Err(error) = project.attach_node_to_container(container, node_id) {
        log::warn!("Cannot add Node to {container:?}: {error}");
        return None;
    }
    // A collapsed root has no visible expanded parent to receive a Node.
    // Expand it after the atomic attachment so every successfully created Node
    // is immediately projected by `build_snarl`.
    if let NodeContainer::Composition(composition_id) = container {
        if let Some(composition) = project.get_composition_mut(composition_id) {
            composition.ui_collapsed = false;
        }
    }
    Some(container)
}

pub(super) fn place_node_in_free_slot(
    project: &mut Project,
    node_id: Uuid,
    container: NodeContainer,
    desired: egui::Pos2,
    dependencies: &[Uuid],
) -> Option<egui::Pos2> {
    let (container_position, container_size, node_ids) = container_geometry(project, container)?;
    let node_size = estimated_node_size(project, node_id);
    let dependency_anchor = dependencies
        .iter()
        .filter_map(|dependency_id| project.get_node(*dependency_id))
        .map(|dependency| dependency.ui_position)
        .collect::<Vec<_>>();
    let mut anchor = if dependency_anchor.is_empty() {
        desired
    } else {
        let count = dependency_anchor.len() as f32;
        egui::pos2(
            dependency_anchor
                .iter()
                .map(|position| position[0])
                .sum::<f32>()
                / count
                + estimated_node_width()
                + AUTO_LAYOUT_COLUMN_GAP,
            dependency_anchor
                .iter()
                .map(|position| position[1])
                .sum::<f32>()
                / count,
        )
    };
    let min = egui::pos2(
        container_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        container_position[1]
            + match container {
                NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
                NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
                NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
            },
    );
    let max_x = (container_position[0] + container_size[0] - AUTO_LAYOUT_TRACK_RIGHT - node_size.x)
        .max(min.x);
    // A dependency-derived position expresses graph order, so preserve it and
    // let `ensure_container_hierarchy_contains` grow the owning containers.
    // Clamping it to a container that is still sized for the old children can
    // place the new dependent Node to the *left* of its source. Pointer-based
    // placement, on the other hand, should remain within the current bounds.
    anchor.x = if dependency_anchor.is_empty() {
        anchor.x.clamp(min.x, max_x)
    } else {
        anchor.x.max(min.x)
    };
    anchor.y = anchor.y.max(min.y);

    let mut occupied = node_ids
        .iter()
        .filter(|child_id| **child_id != node_id)
        .filter_map(|child_id| {
            let child = project.get_node(*child_id)?;
            Some(egui::Rect::from_min_size(
                egui::pos2(child.ui_position[0], child.ui_position[1]),
                estimated_node_size(project, *child_id),
            ))
        })
        .collect::<Vec<_>>();
    occupied.extend(immediate_child_rects(
        project,
        &AutoLayoutPlan::default(),
        container,
    ));
    let mut candidate = egui::Rect::from_min_size(anchor, node_size);
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| candidate.expand(4.0).intersects(**other))
            .map(|other| other.bottom() + AUTO_LAYOUT_ROW_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(anchor.x, next_y), node_size);
    }

    project.get_node_mut(node_id)?.ui_position = [candidate.min.x, candidate.min.y];
    ensure_container_hierarchy_contains(project, container, candidate);
    Some(candidate.min)
}

pub(super) fn container_geometry(
    project: &Project,
    container: NodeContainer,
) -> Option<([f32; 2], [f32; 2], Vec<Uuid>)> {
    match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
        NodeContainer::Clip(id) => project
            .get_clip(id)
            .map(|item| (item.ui_position, item.ui_size, item.node_ids.clone())),
    }
}

/// Resolves the deepest visible container chrome for pointer-only creation and
/// insertion callers. Those callers intentionally accept headers. Geometry
/// reparenting uses `reparent_container_geometries` and content-only target
/// evaluation instead, so neither a collapsed header nor its stored body can
/// acquire an existing Node.
pub(super) fn node_container_at_position(
    project: &Project,
    composition_id: Uuid,
    position: egui::Pos2,
) -> Option<NodeContainer> {
    let composition = project.get_composition(composition_id)?;
    if composition.ui_collapsed {
        return Some(NodeContainer::Composition(composition_id));
    }
    for track_id in composition.track_ids.iter().rev() {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        if !container_visual(project, PortOwner::Track(*track_id))
            .is_some_and(|visual| visual.rect().contains(position))
        {
            continue;
        }
        if !track.ui_collapsed {
            for clip_id in track.clip_ids.iter().rev() {
                if container_visual(project, PortOwner::Clip(*clip_id))
                    .is_some_and(|visual| visual.rect().contains(position))
                {
                    return Some(NodeContainer::Clip(*clip_id));
                }
            }
        }
        return Some(NodeContainer::Track(*track_id));
    }
    Some(NodeContainer::Composition(composition_id))
}

pub(super) fn create_clip_at_free_slot(
    project: &mut Project,
    desired: egui::Pos2,
    composition_id: Uuid,
    name: &str,
) -> Option<Uuid> {
    let composition = project.get_composition(composition_id)?;
    let track_id = composition
        .track_ids
        .iter()
        .rev()
        .find(|track_id| {
            project.get_track(**track_id).is_some_and(|track| {
                container_rect(track.ui_position, track.ui_size).contains(desired)
            })
        })
        .copied()
        .or_else(|| composition.track_ids.first().copied())?;
    let track = project.get_track(track_id)?.clone();
    let mut clip = library::model::Clip::new(name, 0.0, 5.0);
    let size = egui::vec2(clip.ui_size[0], clip.ui_size[1]);
    let min = egui::pos2(
        track.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT,
        track.ui_position[1] + AUTO_LAYOUT_TRACK_TOP,
    );
    let mut candidate =
        egui::Rect::from_min_size(egui::pos2(desired.x.max(min.x), desired.y.max(min.y)), size);
    for existing_id in &track.clip_ids {
        let Some(existing) = project.get_clip(*existing_id) else {
            continue;
        };
        let existing_rect = container_rect(existing.ui_position, existing.ui_size);
        if candidate.intersects(existing_rect) {
            candidate = candidate.translate(egui::vec2(
                0.0,
                existing_rect.bottom() - candidate.top() + AUTO_LAYOUT_ROW_GAP,
            ));
        }
    }
    clip.ui_position = [candidate.min.x, candidate.min.y];
    let clip_id = clip.id;
    project.add_clip(clip);
    if let Err(error) = project.attach_clip_to_track(track_id, clip_id) {
        project.remove_clip(clip_id);
        log::warn!("Cannot add Clip to Track: {error}");
        return None;
    }
    ensure_container_hierarchy_contains(project, NodeContainer::Track(track_id), candidate);
    Some(clip_id)
}

pub(super) fn create_track_at_free_slot(
    project: &mut Project,
    desired: egui::Pos2,
    composition_id: Uuid,
    name: &str,
) -> Option<Uuid> {
    let composition = project.get_composition(composition_id)?.clone();
    let mut track = library::model::Track::new(name);
    let size = egui::vec2(track.ui_size[0], track.ui_size[1]);
    let min = egui::pos2(
        composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_LEFT,
        composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP,
    );
    let mut candidate =
        egui::Rect::from_min_size(egui::pos2(desired.x.max(min.x), desired.y.max(min.y)), size);
    let occupied = composition
        .track_ids
        .iter()
        .filter_map(|track_id| project.get_track(*track_id))
        .map(|track| {
            egui::Rect::from_min_size(
                egui::pos2(track.ui_position[0], track.ui_position[1]),
                egui::vec2(track.ui_size[0], track.ui_size[1]),
            )
        })
        .collect::<Vec<_>>();
    loop {
        let next_y = occupied
            .iter()
            .filter(|other| candidate.expand(8.0).intersects(**other))
            .map(|other| other.bottom() + AUTO_LAYOUT_TRACK_GAP)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            break;
        };
        candidate = egui::Rect::from_min_size(egui::pos2(candidate.min.x, next_y), size);
    }
    track.ui_position = [candidate.min.x, candidate.min.y];
    let track_id = track.id;
    if let Err(error) = project.add_track(track) {
        log::warn!("Cannot add Track to project: {error}");
        return None;
    }
    if let Err(error) = project.attach_track_to_composition(composition_id, track_id) {
        project.remove_track(track_id);
        log::warn!("Cannot add track to composition: {error}");
        return None;
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size[0] = composition.ui_size[0]
            .max(candidate.right() - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT);
        composition.ui_size[1] = composition.ui_size[1]
            .max(candidate.bottom() - composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_BOTTOM);
    }
    Some(track_id)
}
