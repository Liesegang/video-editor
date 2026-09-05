//! One authoritative constructor for basic Timeline clips created by editor
//! surfaces. Timeline menus and Preview tools differ only in placement input.

use std::collections::HashMap;

use library::editor::{AppearanceOperationFactory, TimelineEditorService};
use library::model::authoring::{
    AuthoringProject, MediaTime, ShapeKind, ShapeSource, SourceRef, TimelineId, TimelineInterval,
    TimelineItemId,
};
use library::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};
use library::model::property::{Property, PropertyMap, PropertyValue, Vec2};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BasicClipKind {
    Text,
    Rectangle,
    Ellipse,
    Path,
    Solid,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BasicClipPlacement {
    pub position: Option<[f64; 2]>,
    pub size: Option<[f64; 2]>,
    pub path: Option<PathValue>,
}

pub(crate) fn create_basic_clip(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    state: &AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    kind: BasicClipKind,
    placement: BasicClipPlacement,
) -> Result<TimelineItemId, String> {
    let default_fill =
        || AppearanceOperationFactory::create(plugins, "fill").map_err(|error| error.to_string());
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
    let selected_track = state
        .selection
        .primary()
        .and_then(|selection| match selection {
            AuthoringSelection::Track(track_id) => Some(track_id),
            AuthoringSelection::Item(item_id) => {
                project.items.get(&item_id).map(|item| item.track_id)
            }
            _ => None,
        });
    let track_id = selected_track
        .filter(|track_id| {
            project
                .tracks
                .get(track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
        })
        .or_else(|| timeline.track_order.first().copied())
        .ok_or_else(|| "Add a Track before creating a clip".to_string())?;
    let start = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)?;
    let duration = MediaTime::new(5, 1)?;
    let default_size = [100.0, 100.0];
    let size = placement.size.unwrap_or(default_size);
    if !size
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
    {
        return Err("Clip dimensions must be positive and finite".to_string());
    }
    let (name, source) = match kind {
        BasicClipKind::Text => (
            "Text",
            SourceRef::Text {
                text: "Text".to_string(),
                appearance_operations: vec![default_fill()?],
                ensemble_operations: Vec::new(),
            },
        ),
        BasicClipKind::Rectangle | BasicClipKind::Ellipse => {
            let shape_kind = if kind == BasicClipKind::Rectangle {
                ShapeKind::Rectangle
            } else {
                ShapeKind::Ellipse
            };
            let name = if kind == BasicClipKind::Rectangle {
                "Rectangle"
            } else {
                "Ellipse"
            };
            (
                name,
                SourceRef::Shape {
                    shape: ShapeSource {
                        shape_kind,
                        parameters: HashMap::from([
                            ("width".to_string(), PropertyValue::from(size[0])),
                            ("height".to_string(), PropertyValue::from(size[1])),
                        ]),
                        appearance_operations: vec![default_fill()?],
                    },
                },
            )
        }
        BasicClipKind::Path => {
            let path = placement.path.unwrap_or(default_path()?);
            (
                "Path",
                SourceRef::Shape {
                    shape: ShapeSource {
                        shape_kind: ShapeKind::Path,
                        parameters: HashMap::from([
                            ("path".to_string(), PropertyValue::Path(path)),
                            ("width".to_string(), PropertyValue::from(size[0])),
                            ("height".to_string(), PropertyValue::from(size[1])),
                        ]),
                        appearance_operations: vec![default_fill()?],
                    },
                },
            )
        }
        BasicClipKind::Solid => (
            "Solid",
            SourceRef::Solid {
                color: library::model::frame::color::Color::black(),
            },
        ),
    };
    let mut authored_properties = PropertyMap::new();
    if let Some([x, y]) = placement.position {
        if !x.is_finite() || !y.is_finite() {
            return Err("Clip position must be finite".to_string());
        }
        authored_properties.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(x),
                y: OrderedFloat(y),
            })),
        );
    }
    service
        .add_item_with_authored_properties(
            track_id,
            name.to_string(),
            source,
            TimelineInterval::new(start, duration)?,
            super::panels::timeline::geometry::next_layer(project, track_id),
            authored_properties,
        )
        .map(|(item_id, _)| item_id)
        .map_err(|error| error.to_string())
}

fn default_path() -> Result<PathValue, String> {
    PathValue::new(
        FillRule::NonZero,
        vec![PathContour::new(
            PathPoint::new(0.0, 0.0),
            vec![
                PathSegment::line(PathPoint::new(160.0, 0.0)),
                PathSegment::line(PathPoint::new(160.0, 90.0)),
                PathSegment::line(PathPoint::new(0.0, 90.0)),
            ],
            true,
        )],
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_text_creation_owns_source_and_position_in_one_undo_step() {
        let plugins = PluginManager::default();
        let service = TimelineEditorService::create_default("Canvas Text").unwrap();
        let project = service.snapshot().unwrap();
        let mut state = AuthoringUiState::new(project.root_timeline_id);
        state
            .selection
            .replace(AuthoringSelection::Timeline(project.root_timeline_id));
        let before = service.revision().unwrap();

        let item_id = create_basic_clip(
            &project,
            project.root_timeline_id,
            &state,
            &service,
            &plugins,
            BasicClipKind::Text,
            BasicClipPlacement {
                position: Some([240.0, 135.0]),
                ..Default::default()
            },
        )
        .unwrap();

        let created = service.snapshot().unwrap();
        assert_eq!(service.revision().unwrap().get(), before.get() + 1);
        assert!(matches!(
            created.items[&item_id].source,
            SourceRef::Text { .. }
        ));
        assert_eq!(
            created.items[&item_id]
                .authored_properties
                .get("position")
                .and_then(Property::value),
            Some(&PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(240.0),
                y: OrderedFloat(135.0),
            }))
        );
        service.undo().unwrap().expect("atomic creation Undo");
        assert!(!service.snapshot().unwrap().items.contains_key(&item_id));
    }

    #[test]
    fn canvas_primitive_creation_keeps_drawn_size_and_position_atomic() {
        let plugins = PluginManager::default();
        let service = TimelineEditorService::create_default("Canvas Rectangle").unwrap();
        let project = service.snapshot().unwrap();
        let state = AuthoringUiState::new(project.root_timeline_id);

        let item_id = create_basic_clip(
            &project,
            project.root_timeline_id,
            &state,
            &service,
            &plugins,
            BasicClipKind::Rectangle,
            BasicClipPlacement {
                position: Some([12.0, 18.0]),
                size: Some([320.0, 180.0]),
                path: None,
            },
        )
        .unwrap();

        let created = service.snapshot().unwrap();
        let SourceRef::Shape { shape } = &created.items[&item_id].source else {
            panic!("Rectangle source");
        };
        assert_eq!(shape.shape_kind, ShapeKind::Rectangle);
        assert_eq!(shape.parameters["width"], PropertyValue::from(320.0));
        assert_eq!(shape.parameters["height"], PropertyValue::from(180.0));
        assert_eq!(shape.appearance_operations.len(), 1);
        assert_eq!(
            shape.appearance_operations[0].operation.component_id,
            "fill"
        );
        service.undo().unwrap().expect("atomic creation Undo");
        assert!(!service.snapshot().unwrap().items.contains_key(&item_id));
    }
}
