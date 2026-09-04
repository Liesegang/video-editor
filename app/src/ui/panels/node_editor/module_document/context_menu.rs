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
        let graph_position = transform.inverse() * position;
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
                    let items = super::menu::module_node_menu_items(plugins);
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
}
