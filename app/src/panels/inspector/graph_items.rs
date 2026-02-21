//! Shared types and helpers for graph-based inspector items (effects, styles, ensemble).

use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::command::history::HistoryManager;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::project::connection::PinId;
use library::project::graph_analysis;
use library::project::property::PropertyMap;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Lightweight info about a graph-based node for UI display.
/// Replaces GraphEffectInfo, GraphEnsembleInfo, and GraphStyleInfo.
pub(super) struct GraphNodeInfo {
    pub(super) node_id: Uuid,
    pub(super) type_id: String,
    pub(super) display_name: String,
    pub(super) properties: PropertyMap,
}

/// Collect graph-based nodes associated with a clip, using the given ID retrieval function.
pub(super) fn collect_graph_nodes(
    project: &Arc<RwLock<library::project::project::Project>>,
    project_service: &mut ProjectService,
    clip_id: Uuid,
    get_ids: impl Fn(&library::project::project::Project, Uuid) -> Vec<Uuid>,
) -> Vec<GraphNodeInfo> {
    if let Ok(proj) = project.read() {
        let ids = get_ids(&proj, clip_id);
        ids.into_iter()
            .filter_map(|node_id| {
                let node = proj.get_graph_node(node_id)?;
                let type_id = node.type_id.clone();
                let display_name = project_service
                    .get_plugin_manager()
                    .get_node_type(&type_id)
                    .map(|def| def.display_name.clone())
                    .unwrap_or_else(|| type_id.clone());
                Some(GraphNodeInfo {
                    node_id,
                    type_id,
                    display_name,
                    properties: node.properties.clone(),
                })
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Configuration for adding a node into a chain (effector, decorator, or style).
pub(super) struct ChainConfig {
    category_prefix: &'static str,
    input_pin_name: &'static str,
    output_pin_name: &'static str,
}

impl ChainConfig {
    pub(super) const EFFECTOR: Self = Self {
        category_prefix: "effector",
        input_pin_name: "shape_in",
        output_pin_name: "shape_out",
    };
    pub(super) const DECORATOR: Self = Self {
        category_prefix: "decorator",
        input_pin_name: "shape_in",
        output_pin_name: "shape_out",
    };
    pub(super) const STYLE: Self = Self {
        category_prefix: "style",
        input_pin_name: "style_in",
        output_pin_name: "style_out",
    };
}

/// Add a graph node to a chain, handling existing connections.
fn add_node_to_chain(
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<library::project::project::Project>>,
    track_id: Uuid,
    clip_id: Uuid,
    type_name: &str,
    config: &ChainConfig,
    needs_refresh: &mut bool,
) {
    let graph_type_id = format!("{}.{}", config.category_prefix, type_name);
    match project_service.add_graph_node(track_id, &graph_type_id) {
        Ok(new_node_id) => {
            let clip_pin = PinId::new(clip_id, config.input_pin_name);

            // Check if clip's input already has a connection
            let existing_conn = project.read().ok().and_then(|proj| {
                graph_analysis::get_input_connection(&proj, &clip_pin)
                    .map(|c| (c.id, c.from.clone()))
            });

            if let Some((conn_id, prev_from)) = existing_conn {
                // Chain: disconnect old, connect old→new input, new→clip
                let _ = project_service.remove_graph_connection(conn_id);
                let _ = project_service.add_graph_connection(
                    prev_from,
                    PinId::new(new_node_id, config.input_pin_name),
                );
            }

            let from = PinId::new(new_node_id, config.output_pin_name);
            if let Err(e) = project_service.add_graph_connection(from, clip_pin) {
                log::error!("Failed to connect {}: {}", config.category_prefix, e);
            }
            drop(history_manager.begin_mutation(project));
            *needs_refresh = true;
        }
        Err(e) => {
            log::error!("Failed to add {} graph node: {}", config.category_prefix, e);
        }
    }
}

/// Render an add button for a chain-based plugin category (effector, decorator, or style).
pub(super) fn render_chain_add_button(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<library::project::project::Project>>,
    track_id: Uuid,
    clip_id: Uuid,
    config: &ChainConfig,
    get_available: fn(&library::plugin::PluginManager) -> Vec<String>,
    get_label: fn(&library::plugin::PluginManager, &str) -> String,
    needs_refresh: &mut bool,
) {
    use super::properties::render_add_button;
    render_add_button(ui, |ui| {
        let plugin_manager = project_service.get_plugin_manager();
        for type_name in get_available(&plugin_manager) {
            let label = get_label(&plugin_manager, &type_name);
            if ui.button(label).clicked() {
                add_node_to_chain(
                    project_service,
                    history_manager,
                    project,
                    track_id,
                    clip_id,
                    &type_name,
                    config,
                    needs_refresh,
                );
                ui.close();
            }
        }
    });
}

/// Render a single graph-based item (effect, effector, decorator, or style)
/// with a collapsible header and property grid.
pub(super) fn render_graph_node_item(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<library::project::project::Project>>,
    clip_id: Uuid,
    item: &GraphNodeInfo,
    current_time: f64,
    fps: f64,
    context: &PropertyRenderContext,
    needs_refresh: &mut bool,
    id_prefix: &str,
    default_open: bool,
) {
    let id = ui.make_persistent_id(format!("{}_{}", id_prefix, item.node_id));
    let state = CollapsingState::load_with_default_open(ui.ctx(), id, default_open);

    let mut remove_clicked = false;
    let header_res = state.show_header(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&item.display_name).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("X").clicked() {
                    remove_clicked = true;
                }
            });
        });
    });

    if remove_clicked {
        if let Err(e) = project_service.remove_graph_node(item.node_id) {
            log::error!("Failed to remove {} node: {}", id_prefix, e);
        } else {
            drop(history_manager.begin_mutation(project));
            *needs_refresh = true;
        }
    }

    header_res.body(|ui| {
        let defs = project_service
            .get_plugin_manager()
            .get_node_type(&item.type_id)
            .map(|def| def.default_properties.clone())
            .unwrap_or_default();

        let item_actions = render_inspector_properties_grid(
            ui,
            format!("{}_grid_{}", id_prefix, item.node_id),
            &item.properties,
            &defs,
            project_service,
            context,
            fps,
        );

        let item_props = item.properties.clone();
        let mut ctx = ActionContext::new(project_service, history_manager, clip_id, current_time);
        if ctx.handle_actions(item_actions, PropertyTarget::GraphNode(item.node_id), |n| {
            item_props.get(n).cloned()
        }) {
            *needs_refresh = true;
        }
    });
}
