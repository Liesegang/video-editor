use eframe::egui;
use library::model::property::{PropertyDefinition, PropertyValue};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    PORT_ROW_HEIGHT, PinDefinition, clipped_qa_rect, qa_rect_metadata,
};
#[cfg(test)]
use crate::ui::panels::node_editor::{capture_test_metadata, capture_test_rect};
use crate::ui::widgets::property_drag_value::FloatDragValueConfig;
use crate::ui::widgets::vector_drag_value::{VectorAxisResponse, vector_drag_values};

use super::ProjectNodeViewer;

pub(super) struct RenderedVectorInput {
    pub(super) response: egui::Response,
    pub(super) axes: Vec<VectorAxisResponse>,
    pub(super) changed: bool,
    pub(super) finished: bool,
    pub(super) control_kind: &'static str,
}

pub(super) fn render(
    ui: &mut egui::Ui,
    definition: Option<&PropertyDefinition>,
    value: &mut PropertyValue,
) -> Option<RenderedVectorInput> {
    let config = definition
        .and_then(FloatDragValueConfig::from_definition)
        .unwrap_or(FloatDragValueConfig {
            speed: 0.1,
            suffix: String::new(),
            hard_min: None,
            hard_max: None,
        });
    let (control_kind, group) = match value {
        PropertyValue::Vec2(value) => (
            "vec2",
            vector_drag_values(
                ui,
                &config,
                &mut [("X", &mut value.x.0), ("Y", &mut value.y.0)],
                PORT_ROW_HEIGHT - 2.0,
            ),
        ),
        PropertyValue::Vec3(value) => (
            "vec3",
            vector_drag_values(
                ui,
                &config,
                &mut [
                    ("X", &mut value.x.0),
                    ("Y", &mut value.y.0),
                    ("Z", &mut value.z.0),
                ],
                PORT_ROW_HEIGHT - 2.0,
            ),
        ),
        PropertyValue::Vec4(value) => (
            "vec4",
            vector_drag_values(
                ui,
                &config,
                &mut [
                    ("X", &mut value.x.0),
                    ("Y", &mut value.y.0),
                    ("Z", &mut value.z.0),
                    ("W", &mut value.w.0),
                ],
                PORT_ROW_HEIGHT - 2.0,
            ),
        ),
        _ => return None,
    };
    Some(RenderedVectorInput {
        response: group.response,
        axes: group.axes,
        changed: group.changed,
        finished: group.finished,
        control_kind,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "QA records the exact Node, property, port, timing, metadata, and projected axis geometry"
)]
pub(super) fn register_components(
    viewer: &ProjectNodeViewer<'_>,
    node_id: Uuid,
    property_key: &str,
    pin: &PinDefinition,
    definition: Option<&PropertyDefinition>,
    connected: bool,
    property_time: f64,
    components: Vec<VectorAxisResponse>,
) {
    for component in components {
        viewer.record_body_response(&component.response);
        let axis = component.axis.to_ascii_lowercase();
        let component_id =
            format!("node_editor.property_component.node:{node_id}:{property_key}:{axis}");
        let unclipped_rect = *viewer.to_global * component.response.rect;
        let rect = clipped_qa_rect(unclipped_rect, *viewer.canvas_clip);
        #[cfg(test)]
        {
            capture_test_rect(&component_id, rect);
            capture_test_metadata(
                &component_id,
                &serde_json::json!({
                    "node_id": node_id,
                    "property": property_key,
                    "axis": axis,
                    "value": component.value,
                }),
            );
        }
        crate::qa::register_component_with_metadata(
            component_id,
            "node_vector_component_control",
            rect,
            component.response.enabled(),
            Some(serde_json::json!({
                "node_id": node_id,
                "property": property_key,
                "axis": axis,
                "port": pin.key,
                "connected": connected,
                "control_kind": "float",
                "current_time": property_time,
                "value": component.value,
                "descriptor_available": definition.is_some(),
                "definition": definition.map(
                    crate::ui::panels::inspector::properties::property_definition_metadata
                ),
                "unclipped_rect": qa_rect_metadata(unclipped_rect),
                "visible_in_canvas": rect.is_positive(),
            })),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::HistoryManager;
    use crate::state::context_types::{NodeEditorPendingEdit, NodeEditorState};
    use crate::ui::panels::node_editor::commands::{
        NodeEdit, QueuedNodeEdit, apply_queued_node_edits,
    };
    use crate::ui::panels::node_editor::test_fixture::fixture;
    use library::model::project::{NodeContainer, PortOwner};
    use library::model::property::Vec2;
    use library::plugin::{
        PluginManager, SHAPE_TRANSFORM_COMPONENT_ID, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
    };
    use ordered_float::OrderedFloat;

    fn transform_position_definition(plugins: &PluginManager) -> PropertyDefinition {
        plugins
            .operation_descriptor(
                TRANSFORM_CATEGORY,
                SHAPE_TRANSFORM_COMPONENT_ID,
                TRANSFORM_APPLY_OPERATION,
            )
            .expect("built-in Shape Transform descriptor")
            .properties()
            .iter()
            .find(|definition| definition.name() == "position")
            .expect("Shape Transform position definition")
            .clone()
    }

    #[test]
    fn node_vector_component_responds_to_real_pointer_drag() {
        let plugins = PluginManager::default();
        let definition = transform_position_definition(&plugins);
        let mut value = definition.default_value().clone();
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 160.0));
        let mut x_rect = None;
        let mut changed = false;
        let mut finished = false;

        for frame in 0..2 {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let rendered = render(ui, Some(&definition), &mut value)
                            .expect("Vec2 position renders as an editable vector");
                        x_rect = rendered
                            .axes
                            .iter()
                            .find(|axis| axis.axis == "X")
                            .map(|axis| axis.response.rect);
                    });
                },
            ));
        }

        let start = x_rect.expect("rendered X component").center();
        let end = start + egui::vec2(48.0, 0.0);
        let event_frames = [
            vec![egui::Event::PointerMoved(start)],
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ];
        for (offset, events) in event_frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some((offset + 2) as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let rendered = render(ui, Some(&definition), &mut value)
                            .expect("Vec2 position stays editable during drag");
                        changed |= rendered.changed;
                        finished |= rendered.finished;
                    });
                },
            ));
        }

        let PropertyValue::Vec2(position) = value else {
            panic!("position must stay a Vec2");
        };
        assert!(changed, "real X-axis pointer drag must report a change");
        assert!(finished, "pointer release must finish the vector gesture");
        assert!(position.x.into_inner() > 0.0);
        assert_eq!(position.y, OrderedFloat(0.0), "X drag must preserve Y");
    }

    #[test]
    fn vector_edit_commits_one_authoritative_project_history_gesture() {
        let (mut project, _, _, clip_id, _, _) = fixture();
        let plugins = PluginManager::default();
        let transform = plugins
            .create_shape_transform_operation_node()
            .expect("built-in Shape Transform node");
        let transform_id = transform.id;
        project.add_node(transform);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), transform_id)
            .expect("attach Shape Transform to fixture clip");

        let initial = project.clone();
        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        let mut state = NodeEditorState::default();
        let pending = NodeEditorPendingEdit {
            owner: PortOwner::Node(transform_id),
            key: "position".to_string(),
        };
        for (x, y) in [(12.0, -3.0), (48.0, 7.0)] {
            assert!(apply_queued_node_edits(
                &mut project,
                vec![QueuedNodeEdit::Continuous {
                    pending: pending.clone(),
                    edit: Some(NodeEdit::SetProperty {
                        owner: PortOwner::Node(transform_id),
                        key: "position".to_string(),
                        time: 0.0,
                        value: PropertyValue::Vec2(Vec2 {
                            x: OrderedFloat(x),
                            y: OrderedFloat(y),
                        }),
                    }),
                    finished: false,
                }],
                &mut history,
                &mut state,
            ));
            assert_eq!(history.undo_depth(), 1, "drag updates stay uncommitted");
        }
        assert!(!apply_queued_node_edits(
            &mut project,
            vec![QueuedNodeEdit::Continuous {
                pending,
                edit: None,
                finished: true,
            }],
            &mut history,
            &mut state,
        ));

        assert_eq!(
            project
                .get_node(transform_id)
                .expect("edited transform")
                .properties()
                .get("position")
                .expect("position property")
                .evaluate_at(0.0)
                .expect("position evaluates"),
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(48.0),
                y: OrderedFloat(7.0),
            })
        );
        let edited = project.clone();
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&edited), Some(initial.clone()));
        assert_eq!(history.redo(&initial), Some(edited));
    }
}
