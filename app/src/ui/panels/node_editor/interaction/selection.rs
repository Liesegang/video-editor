use crate::state::context_types::{
    NodeEditorCanvasMarqueeGesture, NodeEditorState, SelectionTarget,
};
use crate::ui::panels::node_editor::GraphItem;
use eframe::egui;
use egui_snarl::Snarl;
use library::model::project::PortOwner;
use uuid::Uuid;

const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;

pub(in crate::ui::panels::node_editor) fn captured_snarl_drag_node(
    context: &egui::Context,
    snarl: &Snarl<GraphItem>,
    snarl_id: egui::Id,
) -> Option<Uuid> {
    captured_snarl_drag_target(context, snarl, snarl_id)?.node_id()
}

pub(in crate::ui::panels::node_editor) fn captured_snarl_drag_target(
    context: &egui::Context,
    snarl: &Snarl<GraphItem>,
    snarl_id: egui::Id,
) -> Option<SelectionTarget> {
    let dragged_id = context.dragged_id()?;
    snarl.node_ids().find_map(|(node_id, item)| {
        if snarl_id.with(("snarl-node", node_id)).with("frame") != dragged_id {
            return None;
        }
        Some(match *item {
            GraphItem::Node(id) => SelectionTarget::Node(id),
            GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => {
                selection_target_for_owner(owner)
            }
        })
    })
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui::panels::node_editor) enum CanvasSelectionOutcome {
    BlankClick {
        additive: bool,
    },
    Marquee {
        targets: Vec<SelectionTarget>,
        additive: bool,
    },
}

pub(in crate::ui::panels::node_editor) const fn selection_target_for_owner(
    owner: PortOwner,
) -> SelectionTarget {
    match owner {
        PortOwner::Node(id) => SelectionTarget::Node(id),
        PortOwner::Clip(id) => SelectionTarget::Clip(id),
        PortOwner::Track(id) => SelectionTarget::Track(id),
        PortOwner::Composition(id) => SelectionTarget::Composition(id),
    }
}

/// Resolve the topmost transient Snarl hit to one authoritative Project
/// identity. A container control card and all four of its port-anchor frames
/// deliberately carry the same `PortOwner` in this list.
pub(in crate::ui::panels::node_editor) fn logical_hit_owner(
    hits: &[(PortOwner, egui::Rect)],
    graph_position: egui::Pos2,
) -> Option<PortOwner> {
    node_editor_ui::selection::topmost_hit(hits, graph_position)
}

pub(in crate::ui::panels::node_editor) fn selection_after_logical_click(
    current_targets: &[SelectionTarget],
    current_primary: Option<SelectionTarget>,
    clicked: SelectionTarget,
    shift: bool,
) -> (Vec<SelectionTarget>, Option<SelectionTarget>) {
    node_editor_ui::selection::after_click(current_targets, current_primary, clicked, shift)
}

pub(in crate::ui::panels::node_editor) fn selection_after_marquee(
    current_targets: &[SelectionTarget],
    marquee_targets: &[SelectionTarget],
    additive: bool,
) -> (Vec<SelectionTarget>, Option<SelectionTarget>) {
    node_editor_ui::selection::after_marquee(current_targets, marquee_targets, additive)
}

pub(in crate::ui::panels::node_editor) fn canvas_marquee_interaction(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    selection_hits: &[(PortOwner, egui::Rect)],
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    enabled: bool,
    pointer_is_specialized: bool,
) -> Option<CanvasSelectionOutcome> {
    let (primary_pressed, primary_down, primary_released, pointer, shift, alt) =
        ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.primary_down(),
                input.pointer.primary_released(),
                input.pointer.interact_pos(),
                input.modifiers.shift,
                input.modifiers.alt,
            )
        });

    if state.canvas_marquee.is_none()
        && enabled
        && primary_pressed
        && !pointer_is_specialized
        && !alt
    {
        if let Some(position) = pointer.filter(|position| canvas_clip.contains(*position)) {
            let graph_position = to_global.inverse() * position;
            if logical_hit_owner(selection_hits, graph_position).is_none() {
                ui.ctx()
                    .set_dragged_id(ui.make_persistent_id("node_editor_canvas_marquee"));
                state.canvas_marquee = Some(NodeEditorCanvasMarqueeGesture {
                    start: position,
                    current: position,
                    additive: shift,
                    canvas_transform: to_global,
                });
            }
        }
    }

    if let (Some(position), Some(gesture)) = (pointer, state.canvas_marquee.as_mut()) {
        gesture.current = position;
    }

    if let Some(gesture) = state.canvas_marquee.as_ref() {
        let rect = egui::Rect::from_two_pos(gesture.start, gesture.current).intersect(canvas_clip);
        let painter = ui
            .ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("node_editor_canvas_marquee"),
            ))
            .with_clip_rect(canvas_clip);
        painter.rect(
            rect,
            0.0,
            egui::Color32::from_rgba_premultiplied(76, 146, 255, 30),
            egui::Stroke::new(1.0, egui::Color32::from_rgb(105, 165, 255)),
            egui::StrokeKind::Inside,
        );
        crate::qa::register_component_with_metadata(
            "node_editor.marquee",
            "node_editor_marquee",
            rect,
            true,
            Some(serde_json::json!({
                "additive": gesture.additive,
                "start": {"x": gesture.start.x, "y": gesture.start.y},
                "current": {"x": gesture.current.x, "y": gesture.current.y},
            })),
        );
    }

    if primary_released {
        let gesture = state.canvas_marquee.take()?;
        if gesture.start.distance(gesture.current) < MARQUEE_DRAG_THRESHOLD {
            return Some(CanvasSelectionOutcome::BlankClick {
                additive: gesture.additive,
            });
        }
        let selection_rect = egui::Rect::from_two_pos(gesture.start, gesture.current);
        let mut targets = Vec::new();
        for (owner, graph_rect) in selection_hits {
            let target = selection_target_for_owner(*owner);
            if selection_rect.intersects(gesture.canvas_transform * *graph_rect)
                && !targets.contains(&target)
            {
                targets.push(target);
            }
        }
        return Some(CanvasSelectionOutcome::Marquee {
            targets,
            additive: gesture.additive,
        });
    }

    if !primary_down && !primary_released {
        state.canvas_marquee = None;
    }
    None
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn node_selection_after_snarl_click(
    current_targets: &[SelectionTarget],
    current_primary: Option<SelectionTarget>,
    snarl_selected_node_ids: &[Uuid],
    clicked_node_id: Uuid,
    modifiers: egui::Modifiers,
) -> (Vec<SelectionTarget>, Option<SelectionTarget>) {
    // egui-snarl applies Shift/Cmd selection changes before `Snarl::show`
    // returns. Cmd without Shift is its deselect gesture. In that case the
    // clicked Node is intentionally absent from the post-show snapshot; do
    // not mistake that absence for an update race and select it again.
    if modifiers.shift {
        let mut targets = current_targets
            .iter()
            .copied()
            .filter(|target| !matches!(target, SelectionTarget::Node(_)))
            .collect::<Vec<_>>();
        for node_id in snarl_selected_node_ids {
            let target = SelectionTarget::Node(*node_id);
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        let clicked = SelectionTarget::Node(clicked_node_id);
        if !targets.contains(&clicked) {
            targets.push(clicked);
        }
        return (targets, Some(clicked));
    }

    if modifiers.command {
        let clicked = SelectionTarget::Node(clicked_node_id);
        let targets = current_targets
            .iter()
            .copied()
            .filter(|target| *target != clicked)
            .collect::<Vec<_>>();
        let primary = current_primary
            .filter(|target| targets.contains(target))
            .or_else(|| targets.last().copied());
        return (targets, primary);
    }

    if snarl_selected_node_ids.contains(&clicked_node_id) {
        let targets = snarl_selected_node_ids
            .iter()
            .copied()
            .map(SelectionTarget::Node)
            .collect::<Vec<_>>();
        return (targets, Some(SelectionTarget::Node(clicked_node_id)));
    }

    let target = SelectionTarget::Node(clicked_node_id);
    (vec![target], Some(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn run_marquee_frame(
        context: &egui::Context,
        state: &mut NodeEditorState,
        hits: &[(PortOwner, egui::Rect)],
        events: Vec<egui::Event>,
        specialized: bool,
    ) -> Option<CanvasSelectionOutcome> {
        let mut outcome = None;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 220.0));
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    outcome = canvas_marquee_interaction(
                        ui,
                        state,
                        hits,
                        egui::emath::TSTransform::IDENTITY,
                        screen,
                        true,
                        specialized,
                    );
                });
            },
        ));
        outcome
    }

    #[test]
    fn every_container_subregion_resolves_to_one_logical_owner() {
        let id = Uuid::from_u128(0xA11);
        let owner = PortOwner::Track(id);
        let hits = vec![
            (
                owner,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(30.0, 20.0)),
            ),
            (
                owner,
                egui::Rect::from_min_max(egui::pos2(30.0, 0.0), egui::pos2(50.0, 20.0)),
            ),
            (
                owner,
                egui::Rect::from_min_max(egui::pos2(50.0, 0.0), egui::pos2(70.0, 20.0)),
            ),
        ];

        for point in [
            egui::pos2(10.0, 10.0),
            egui::pos2(40.0, 10.0),
            egui::pos2(60.0, 10.0),
        ] {
            assert_eq!(logical_hit_owner(&hits, point), Some(owner));
            assert_eq!(
                selection_target_for_owner(owner),
                SelectionTarget::Track(id)
            );
        }
    }

    #[test]
    fn shift_click_adds_then_toggles_the_same_logical_item() {
        let first = SelectionTarget::Node(Uuid::from_u128(1));
        let container = SelectionTarget::Clip(Uuid::from_u128(2));

        let (targets, primary) =
            selection_after_logical_click(&[first], Some(first), container, true);
        assert_eq!(targets, vec![first, container]);
        assert_eq!(primary, Some(container));

        let (targets, primary) = selection_after_logical_click(&targets, primary, container, true);
        assert_eq!(targets, vec![first]);
        assert_eq!(primary, Some(first));
    }

    #[test]
    fn marquee_deduplicates_container_anchor_hits() {
        let container = SelectionTarget::Track(Uuid::from_u128(3));
        let node = SelectionTarget::Node(Uuid::from_u128(4));
        let (targets, primary) =
            selection_after_marquee(&[], &[container, container, node, container], false);

        assert_eq!(targets, vec![container, node]);
        assert_eq!(primary, Some(node));
    }

    #[test]
    fn blank_primary_drag_runs_marquee_while_specialized_press_does_not() {
        let owner = PortOwner::Node(Uuid::from_u128(5));
        let hits = [(
            owner,
            egui::Rect::from_min_max(egui::pos2(90.0, 90.0), egui::pos2(120.0, 120.0)),
        )];
        let context = egui::Context::default();
        let mut state = NodeEditorState::default();
        let start = egui::pos2(20.0, 20.0);
        let end = egui::pos2(140.0, 140.0);

        assert!(run_marquee_frame(
            &context,
            &mut state,
            &hits,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true)
            ],
            false,
        )
        .is_none());
        assert!(state.canvas_marquee.is_some());
        assert!(run_marquee_frame(
            &context,
            &mut state,
            &hits,
            vec![egui::Event::PointerMoved(end)],
            false,
        )
        .is_none());
        assert_eq!(
            run_marquee_frame(
                &context,
                &mut state,
                &hits,
                vec![pointer_button(end, false)],
                false,
            ),
            Some(CanvasSelectionOutcome::Marquee {
                targets: vec![SelectionTarget::Node(owner.id())],
                additive: false,
            })
        );
        assert!(state.canvas_marquee.is_none());

        let specialized_context = egui::Context::default();
        let mut specialized_state = NodeEditorState::default();
        assert!(run_marquee_frame(
            &specialized_context,
            &mut specialized_state,
            &[],
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true)
            ],
            true,
        )
        .is_none());
        assert!(specialized_state.canvas_marquee.is_none());
    }
}
