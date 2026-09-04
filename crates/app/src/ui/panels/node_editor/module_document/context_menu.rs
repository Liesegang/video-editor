use super::*;
use crate::state::node_editor::ModuleCreateMenuState;
use crate::ui::widgets::searchable_context_menu::{
    register_searchable_popup_qa, searchable_menu_click_is_outside, searchable_popup_placement,
    show_searchable_items_with_qa, show_searchable_popup_frame,
};

pub(super) fn show_module_create_menu(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    plugins: &PluginManager,
    definition: &ModuleDefinition,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
    node_rects: &[egui::Rect],
) -> Option<(ModuleNodeCreateRequest, egui::Pos2)> {
    let (secondary_clicked, pointer_position, open_time) = ui.input(|input| {
        (
            input.pointer.secondary_clicked(),
            input.pointer.interact_pos(),
            input.time,
        )
    });
    update_for_secondary_click(
        &mut state.create_menu,
        secondary_clicked,
        pointer_position,
        viewport,
        node_rects,
        transform,
        open_time,
    );

    let mut selected = None;
    let mut should_close = false;
    if let Some(context) = state.create_menu.as_ref() {
        let position = context.position;
        let graph_position = visible_creation_position(position, viewport, transform);
        let popup =
            searchable_popup_placement(position, egui::vec2(320.0, 348.0), ui.ctx().content_rect());
        let menu_id = format!("node_editor_add_menu:{}", context.open_time.to_bits());
        let response = egui::Area::new(egui::Id::new("node_editor_context_menu"))
            .order(egui::Order::Foreground)
            .pivot(popup.pivot)
            .fixed_pos(popup.area_anchor)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                show_searchable_popup_frame(ui, popup, |ui| {
                    let items =
                        super::menu::module_node_menu_items(plugins, &definition.host_contract);
                    if let Some(request) = show_searchable_items_with_qa(
                        ui,
                        &menu_id,
                        Some("node_editor.menu.search"),
                        &items,
                    ) {
                        selected = Some((request, graph_position));
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
    if should_close {
        state.create_menu = None;
    }
    selected
}

/// Place a new Node around the invocation point while keeping its initial
/// controls reachable. The mature Snarl surface can measure a more precise
/// size on the next frame; this conservative footprint prevents a context
/// menu near an edge from creating every output port off-screen.
fn visible_creation_position(
    pointer: egui::Pos2,
    viewport: egui::Rect,
    transform: egui::emath::TSTransform,
) -> egui::Pos2 {
    const SCREEN_MARGIN: f32 = 12.0;
    const CREATED_NODE_WIDTH: f32 = 420.0;
    const CREATED_NODE_HEIGHT: f32 = 220.0;

    let scale = transform.scaling.abs().max(f32::EPSILON);
    let available =
        (viewport.size() - egui::Vec2::splat(SCREEN_MARGIN * 2.0)).max(egui::Vec2::ZERO);
    let footprint = (egui::vec2(CREATED_NODE_WIDTH, CREATED_NODE_HEIGHT) * scale).min(available);
    let minimum = viewport.min + egui::Vec2::splat(SCREEN_MARGIN);
    let maximum = viewport.max - egui::Vec2::splat(SCREEN_MARGIN) - footprint;
    let desired = pointer - footprint * 0.5;
    let screen_position = egui::pos2(
        desired.x.clamp(minimum.x, maximum.x.max(minimum.x)),
        desired.y.clamp(minimum.y, maximum.y.max(minimum.y)),
    );
    transform.inverse() * screen_position
}

fn update_for_secondary_click(
    state: &mut Option<ModuleCreateMenuState>,
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
        *state = None;
        return;
    }
    *state = Some(ModuleCreateMenuState::new(position, open_time));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_surface_prevents_the_blank_canvas_menu() {
        let mut state = None;
        update_for_secondary_click(
            &mut state,
            true,
            Some(egui::pos2(120.0, 120.0)),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(300.0, 300.0)),
            &[egui::Rect::from_min_max(
                egui::pos2(100.0, 100.0),
                egui::pos2(200.0, 200.0),
            )],
            egui::emath::TSTransform::IDENTITY,
            1.0,
        );
        assert!(state.is_none());
    }

    #[test]
    fn creation_near_an_edge_keeps_a_conservative_node_footprint_visible() {
        let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(600.0, 400.0));
        let transform = egui::emath::TSTransform::new(egui::vec2(100.0, 50.0), 0.5);

        let graph_position =
            visible_creation_position(egui::pos2(695.0, 445.0), viewport, transform);
        let screen_position = transform * graph_position;
        let footprint = egui::vec2(420.0, 220.0) * transform.scaling;

        assert!(screen_position.x >= viewport.left() + 12.0);
        assert!(screen_position.y >= viewport.top() + 12.0);
        assert!(screen_position.x + footprint.x <= viewport.right() - 12.0 + f32::EPSILON);
        assert!(screen_position.y + footprint.y <= viewport.bottom() - 12.0 + f32::EPSILON);
    }
}
