use super::*;

impl TimelineEditorService {
    /// Replace the canonical geometry of one authored Path clip.
    ///
    /// Preview gestures call this exactly once on release. The source kind,
    /// placement, item properties, effects, and sibling clips are untouched.
    pub fn set_shape_path(
        &self,
        item_id: TimelineItemId,
        path: crate::model::path::PathValue,
    ) -> Result<ChangeSet, LibraryError> {
        path.validate()
            .map_err(|error| LibraryError::Validation(format!("Invalid Path: {error}")))?;
        self.edit_item(item_id, |item| {
            let SourceRef::Shape { shape } = &mut item.source else {
                return Err(format!("Timeline item {item_id} is not a Shape"));
            };
            if shape.shape_kind != crate::model::authoring::ShapeKind::Path {
                return Err(format!("Timeline item {item_id} is not a Path Shape"));
            }
            shape
                .parameters
                .insert("path".to_string(), PropertyValue::Path(path));
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::{ShapeKind, ShapeSource};
    use crate::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};

    fn path(end_x: f64) -> PathValue {
        PathValue::new(
            FillRule::NonZero,
            vec![PathContour::new(
                PathPoint::new(0.0, 0.0),
                vec![PathSegment::line(PathPoint::new(end_x, 40.0))],
                false,
            )],
        )
        .unwrap()
    }

    #[test]
    fn path_replace_is_one_atomic_undo_step() {
        let service = TimelineEditorService::create_default("Path edit").unwrap();
        let project = service.snapshot().unwrap();
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let original = path(80.0);
        let (item_id, _) = service
            .add_item(
                track_id,
                "Path".to_string(),
                SourceRef::Shape {
                    shape: ShapeSource {
                        shape_kind: ShapeKind::Path,
                        parameters: HashMap::from([(
                            "path".to_string(),
                            PropertyValue::Path(original.clone()),
                        )]),
                    },
                },
                TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 1).unwrap()).unwrap(),
                0,
            )
            .unwrap();
        let before_revision = service.revision().unwrap();
        let replacement = path(140.0);

        service
            .set_shape_path(item_id, replacement.clone())
            .unwrap();

        assert_eq!(service.revision().unwrap().get(), before_revision.get() + 1);
        let changed = service.snapshot().unwrap();
        let SourceRef::Shape { shape } = &changed.items[&item_id].source else {
            panic!("Path source");
        };
        assert_eq!(
            shape.parameters.get("path"),
            Some(&PropertyValue::Path(replacement))
        );
        service.undo().unwrap().expect("Path transaction");
        let undone = service.snapshot().unwrap();
        let SourceRef::Shape { shape } = &undone.items[&item_id].source else {
            panic!("Path source");
        };
        assert_eq!(
            shape.parameters.get("path"),
            Some(&PropertyValue::Path(original))
        );
    }

    #[test]
    fn non_path_shape_is_refused_without_mutation() {
        let service = TimelineEditorService::create_default("Path edit").unwrap();
        let project = service.snapshot().unwrap();
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let (item_id, _) = service
            .add_item(
                track_id,
                "Rectangle".to_string(),
                SourceRef::Shape {
                    shape: ShapeSource {
                        shape_kind: ShapeKind::Rectangle,
                        parameters: HashMap::new(),
                    },
                },
                TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 1).unwrap()).unwrap(),
                0,
            )
            .unwrap();
        let before = service.snapshot().unwrap();

        assert!(service.set_shape_path(item_id, path(40.0)).is_err());
        assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    }
}
