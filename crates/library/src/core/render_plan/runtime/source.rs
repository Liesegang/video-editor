//! Evaluate Timeline-owned media, Text, Shape, and nested source placements.

use super::*;

impl AuthoringFrameEvaluator<'_> {
    pub(super) fn evaluate_item_source(
        &mut self,
        timeline: &Timeline,
        item: &TimelineItem,
        timeline_time: MediaTime,
        local_time: MediaTime,
        instance_path: &InstancePath,
        transition_id: Option<crate::model::authoring::TransitionId>,
    ) -> Result<Option<FrameItem>, LibraryError> {
        match &item.source {
            SourceRef::Asset { asset_id } => self.asset_item(
                item.id.as_uuid(),
                *asset_id,
                local_time,
                timeline.fps.to_f64(),
                timeline_time,
                transition_id,
            ),
            SourceRef::Text {
                text,
                appearance_operations,
                ensemble_operations,
            } => {
                let text = self.effective_text(timeline, item.id, text, instance_path)?;
                let values =
                    self.effective_item_property_values(timeline, item, local_time, instance_path)?;
                let ensemble = match evaluate_text_ensemble(
                    self.plugins,
                    ensemble_operations,
                    local_time.to_seconds_f64(),
                    timeline.fps.to_f64(),
                    (timeline.width, timeline.height),
                )? {
                    crate::model::project::EvalOutput::Produced(ensemble) => ensemble,
                    crate::model::project::EvalOutput::NoOutput => return Ok(None),
                };
                appearance_image(
                    self.plugins,
                    appearance_operations,
                    (timeline.width, timeline.height),
                    local_time.to_seconds_f64(),
                    timeline.fps.to_f64(),
                    |style| {
                        text_item_from_values(
                            item.id.as_uuid(),
                            &text,
                            &values,
                            vec![style],
                            ensemble.clone(),
                            local_time.to_seconds_f64() as f32,
                        )
                    },
                )
            }
            SourceRef::Shape { shape } => {
                let values =
                    self.effective_item_property_values(timeline, item, local_time, instance_path)?;
                let mut shape = shape.clone();
                for key in ["width", "height", "color"] {
                    if let Some(value) = values.get(key) {
                        shape.parameters.insert(key.to_string(), value.clone());
                    }
                }
                appearance_image(
                    self.plugins,
                    &shape.appearance_operations,
                    (timeline.width, timeline.height),
                    local_time.to_seconds_f64(),
                    timeline.fps.to_f64(),
                    |style| shape_item(item.id.as_uuid(), &shape, vec![style]),
                )
            }
            SourceRef::Solid { color } => {
                let values =
                    self.effective_item_property_values(timeline, item, local_time, instance_path)?;
                let color = if values.contains_key("color") {
                    frame_values::required_color(&values, "color", "Solid")?
                } else {
                    color.clone()
                };
                Ok(Some(solid_item(
                    item.id.as_uuid(),
                    timeline.width,
                    timeline.height,
                    crate::model::property::ColorValue::from_straight_srgba8(&color),
                    BlendMode::Normal,
                )))
            }
            SourceRef::Composition(instance) => {
                let nested = self
                    .project
                    .timelines
                    .get(&instance.timeline_id)
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Item {} refers to missing nested Timeline {}",
                            item.id, instance.timeline_id
                        ))
                    })?;
                let Some(nested_time) = map_composition_time(
                    item,
                    nested.duration,
                    &instance.duration_policy,
                    timeline_time,
                )
                .map_err(LibraryError::Validation)?
                else {
                    if let Some(transition_id) = transition_id {
                        return Err(TransitionSourceHandleError {
                            transition_id: transition_id.as_uuid(),
                            item_id: item.id.as_uuid(),
                            timeline_time: timeline_time.to_seconds_f64(),
                            source_time: local_time.to_seconds_f64(),
                            reason:
                                "nested Composition duration policy cannot map this hidden handle"
                                    .to_string(),
                        }
                        .into());
                    }
                    return Ok(None);
                };
                let path = instance_path.nested(item.id);
                self.evaluate_timeline_group(instance.timeline_id, nested_time, &path)
                    .map(Some)
            }
            SourceRef::Module(_) => self.evaluate_module_host(
                ModuleHost::TimelineItem {
                    timeline_id: timeline.id,
                    item_id: item.id,
                },
                timeline.id,
                local_time,
                timeline_time,
                instance_path,
                None,
            ),
        }
    }

    pub(super) fn asset_item(
        &self,
        source_id: uuid::Uuid,
        asset_id: uuid::Uuid,
        source_time: MediaTime,
        evaluation_fps: f64,
        timeline_time: MediaTime,
        transition_id: Option<crate::model::authoring::TransitionId>,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let asset = self
            .project
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} is missing")))?;
        if asset.kind == AssetKind::Audio {
            return Ok(None);
        }
        if matches!(asset.kind, AssetKind::Model3D | AssetKind::Other) {
            return Err(LibraryError::Render(format!(
                "Asset {asset_id} has no visual Timeline renderer"
            )));
        }
        let seconds = source_time.to_seconds_f64();
        if asset.kind == AssetKind::Video && seconds < 0.0 {
            return match transition_id {
                Some(transition_id) => Err(TransitionSourceHandleError {
                    transition_id: transition_id.as_uuid(),
                    item_id: source_id,
                    timeline_time: timeline_time.to_seconds_f64(),
                    source_time: seconds,
                    reason: "source has no media before time zero".to_string(),
                }
                .into()),
                None => Ok(None),
            };
        }
        if asset.kind == AssetKind::Video
            && asset.duration.is_some_and(|duration| seconds >= duration)
        {
            return match transition_id {
                Some(transition_id) => Err(TransitionSourceHandleError {
                    transition_id: transition_id.as_uuid(),
                    item_id: source_id,
                    timeline_time: timeline_time.to_seconds_f64(),
                    source_time: seconds,
                    reason: format!(
                        "source duration ends at {}s",
                        asset.duration.unwrap_or_default()
                    ),
                }
                .into()),
                None => Ok(None),
            };
        }
        if let Some(frame) = asset.source_frame_number_at(seconds, evaluation_fps)
            && !asset.contains_source_frame(frame)
        {
            return match transition_id {
                Some(transition_id) => Err(TransitionSourceHandleError {
                    transition_id: transition_id.as_uuid(),
                    item_id: source_id,
                    timeline_time: timeline_time.to_seconds_f64(),
                    source_time: seconds,
                    reason: format!("source frame {frame} is outside the decodable frame range"),
                }
                .into()),
                None => Ok(None),
            };
        }
        let surface = ImageSurface {
            asset_id: Some(asset.id),
            file_path: asset.path.clone(),
            effects: Vec::new(),
            input_color_space: None,
            output_color_space: None,
            transform: Transform::default(),
        };
        let content = match asset.kind {
            AssetKind::Video => FrameContent::Video {
                surface,
                source_time: seconds,
                stream_index: asset.stream_index,
            },
            AssetKind::Image => FrameContent::Image { surface },
            AssetKind::Audio | AssetKind::Model3D | AssetKind::Other => {
                return Err(LibraryError::Render(format!(
                    "Asset {asset_id} changed type during frame evaluation"
                )));
            }
        };
        Ok(Some(FrameItem::Object(FrameObject {
            source_node_id: source_id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: match (asset.width, asset.height) {
                (Some(width), Some(height)) => {
                    Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32))
                }
                _ => None,
            },
            content,
        })))
    }
}
