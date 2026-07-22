use crate::core::cache::SharedCacheManager;
use crate::core::framing::get_frame_from_project;
use crate::core::rendering::renderer::{
    Affine2D, RenderOutput, Renderer, ShapeRasterRequest, TextRasterRequest,
};
use crate::editor::project_model::ProjectModel;
use crate::error::LibraryError;
use crate::model::frame::entity::{
    FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject,
};
use crate::model::frame::frame::FrameInfo;
use crate::model::frame::transform::Transform;
use crate::plugin::{LoadRequest, PluginManager};
use crate::util::timing::{ScopedTimer, measure_debug};
use std::sync::Arc;

pub struct RenderService<T: Renderer> {
    pub renderer: T,
    cache_manager: SharedCacheManager,
    plugin_manager: Arc<PluginManager>,
}

impl<T: Renderer> RenderService<T> {
    pub fn new(
        renderer: T,
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
    ) -> Self {
        Self {
            renderer,
            plugin_manager,
            cache_manager,
        }
    }

    pub fn render_frame(
        &mut self,
        project_model: &ProjectModel,
        time: f64,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        let frame_info = self.get_frame(project_model, time)?;
        self.render_from_frame_info(&frame_info)
    }

    pub fn render_from_frame_info(
        &mut self,
        frame_info: &FrameInfo,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        self.clear()?;
        let object_count = frame_info.object_count();
        let _timer = ScopedTimer::debug(format!(
            "RenderService::render_frame objects={}",
            object_count
        ));
        let context = RenderContext::root(frame_info);
        self.render_items(
            &frame_info.items,
            &context,
            frame_info.now_time.into_inner(),
        )?;
        measure_debug("RenderService::finalize", || self.renderer.finalize())
    }

    fn render_items(
        &mut self,
        items: &[FrameItem],
        context: &RenderContext,
        current_time: f64,
    ) -> Result<(), LibraryError> {
        for item in items {
            match item {
                FrameItem::Object(object) => {
                    self.render_object(object, context, current_time)?;
                }
                FrameItem::Group(group) => self.render_group(group, context)?,
            }
        }
        Ok(())
    }

    fn render_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
    ) -> Result<(), LibraryError> {
        if group.kind == FrameGroupKind::Composition {
            return self.render_composition_group(group, parent_context);
        }
        if group.kind == FrameGroupKind::ImageTransform {
            return self.render_image_transform_group(group, parent_context);
        }

        let child_context = parent_context.with_transform(&group.transform);
        if !group_requires_isolation(group) {
            return self.render_items(&group.items, &child_context, group.effect_time.into_inner());
        }

        self.renderer.begin_group(
            parent_context.target_width,
            parent_context.target_height,
            &transparent_color(),
        )?;
        let children_result =
            self.render_items(&group.items, &child_context, group.effect_time.into_inner());
        let output_result = self.renderer.end_group();
        children_result?;
        let output = output_result?;
        let output = self.apply_effects(output, &group.effects, group.effect_time.into_inner())?;
        self.renderer.draw_layer_affine_with_blend(
            &output,
            &Affine2D::IDENTITY,
            group.transform.opacity,
            group.blend_mode,
        )
    }

    /// Rasterize the upstream Image subtree before its affine transform,
    /// preserving graph order and treating descendants as one image.
    fn render_image_transform_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
    ) -> Result<(), LibraryError> {
        let width = scaled_dimension(group.width as f64, parent_context.render_scale);
        let height = scaled_dimension(group.height as f64, parent_context.render_scale);
        let child_context = RenderContext::composition(parent_context.render_scale, width, height);

        self.renderer
            .begin_group(width, height, &transparent_color())?;
        let children_result =
            self.render_items(&group.items, &child_context, group.effect_time.into_inner());
        let output_result = self.renderer.end_group();
        children_result?;
        let output = output_result?;

        let pixel_to_local = Affine2D::scale(
            1.0 / parent_context.render_scale,
            1.0 / parent_context.render_scale,
        );
        let transform = parent_context
            .logical_to_target
            .compose(Affine2D::from(&group.transform))
            .compose(pixel_to_local);
        self.renderer.draw_layer_affine_with_blend(
            &output,
            &transform,
            group.transform.opacity,
            group.blend_mode,
        )
    }

    fn render_composition_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
    ) -> Result<(), LibraryError> {
        let width = scaled_dimension(group.width as f64, parent_context.render_scale);
        let height = scaled_dimension(group.height as f64, parent_context.render_scale);
        let child_context = RenderContext::composition(parent_context.render_scale, width, height);

        self.renderer
            .begin_group(width, height, &group.background_color)?;
        let children_result =
            self.render_items(&group.items, &child_context, group.effect_time.into_inner());
        let output_result = self.renderer.end_group();
        children_result?;
        let output = output_result?;
        let output = self.apply_effects(output, &group.effects, group.effect_time.into_inner())?;

        let pixel_to_local = Affine2D::scale(
            1.0 / parent_context.render_scale,
            1.0 / parent_context.render_scale,
        );
        let transform = parent_context
            .logical_to_target
            .compose(Affine2D::from(&group.transform))
            .compose(pixel_to_local);
        self.renderer.draw_layer_affine_with_blend(
            &output,
            &transform,
            group.transform.opacity,
            group.blend_mode,
        )
    }

    fn render_object(
        &mut self,
        frame_object: &FrameObject,
        context: &RenderContext,
        current_time: f64,
    ) -> Result<(), LibraryError> {
        let content = &frame_object.content;

        match content {
            FrameContent::Video {
                surface,
                source_time,
                stream_index,
            } => {
                let request = LoadRequest::VideoFrame {
                    path: surface.file_path.clone(),
                    source_time: *source_time,
                    stream_index: *stream_index,
                    input_color_space: surface.input_color_space.clone(),
                    output_color_space: surface.output_color_space.clone(),
                };
                self.render_media_surface(
                    &request,
                    &surface.file_path,
                    &surface.transform,
                    &surface.effects,
                    context,
                    current_time,
                )
            }
            FrameContent::Image { surface } => {
                let request = LoadRequest::Image {
                    path: surface.file_path.clone(),
                };
                self.render_media_surface(
                    &request,
                    &surface.file_path,
                    &surface.transform,
                    &surface.effects,
                    context,
                    current_time,
                )
            }
            FrameContent::Text {
                text,
                font,
                size,
                styles,
                effects,
                ensemble,
                transform,
            } => {
                let render_transform = context.transform(transform);
                let text_layer = measure_debug(format!("Rasterize text layer '{}'", text), || {
                    self.renderer.rasterize_text_layer(TextRasterRequest {
                        text,
                        size: *size,
                        font_name: font,
                        styles,
                        ensemble: ensemble.as_ref(),
                        transform: render_transform,
                        current_time,
                    })
                })?;
                let final_image = self.apply_effects(text_layer, effects, current_time)?;
                measure_debug(format!("Composite text '{}'", text), || {
                    self.renderer.draw_layer_affine_with_blend(
                        &final_image,
                        &Affine2D::IDENTITY,
                        transform.opacity,
                        crate::model::BlendMode::Normal,
                    )
                })
            }
            FrameContent::Shape {
                path,
                canonical_path,
                styles,
                path_effects,
                effects,
                ensemble,
                transform,
            } => {
                let render_transform = context.transform(transform);
                let shape_layer = measure_debug(format!("Rasterize shape layer {}", path), || {
                    self.renderer.rasterize_shape_layer(ShapeRasterRequest {
                        path_data: path,
                        canonical_path: canonical_path.as_ref(),
                        styles,
                        path_effects,
                        ensemble: ensemble.as_ref(),
                        transform: render_transform,
                    })
                })?;
                let final_image = self.apply_effects(shape_layer, effects, current_time)?;
                measure_debug(format!("Composite shape {}", path), || {
                    self.renderer.draw_layer_affine_with_blend(
                        &final_image,
                        &Affine2D::IDENTITY,
                        transform.opacity,
                        crate::model::BlendMode::Normal,
                    )
                })
            }
            FrameContent::SkSL {
                shader,
                resolution,
                effects,
                transform,
            } => {
                let render_transform = context.transform(transform);
                let sksl_layer = measure_debug("Rasterize SkSL", || {
                    self.renderer.rasterize_sksl_layer(
                        shader,
                        *resolution,
                        current_time as f32,
                        &render_transform,
                    )
                })?;
                let final_image = self.apply_effects(sksl_layer, effects, current_time)?;
                measure_debug("Composite SkSL", || {
                    self.renderer.draw_layer_affine_with_blend(
                        &final_image,
                        &Affine2D::IDENTITY,
                        transform.opacity,
                        crate::model::BlendMode::Normal,
                    )
                })
            }
        }
    }
    pub fn clear(&mut self) -> Result<(), LibraryError> {
        measure_debug("RenderService::clear", || self.renderer.clear())
    }

    fn get_frame(
        &self,
        project_model: &ProjectModel,
        time: f64,
    ) -> Result<FrameInfo, LibraryError> {
        let property_evaluators = self.plugin_manager.get_property_evaluators();

        let project = project_model.project();
        let composition_index = project_model.composition_index();
        let composition = &project.compositions[composition_index];
        let composition_fps = composition.fps;

        // Convert time (f64) to frame_number (u64) using composition_fps
        let frame_number = (time * composition_fps).round() as u64;

        get_frame_from_project(
            project,
            composition_index,
            frame_number, // Pass frame_number (u64)
            1.0,          // Default render_scale to 1.0 for self-managed renders (e.g. export)
            None,
            &property_evaluators,
            &self.plugin_manager,
        )
    }

    fn apply_effects(
        &mut self,
        layer: RenderOutput,
        effects: &[crate::model::frame::effect::ImageEffect],
        current_time: f64,
    ) -> Result<RenderOutput, LibraryError> {
        if effects.is_empty() {
            Ok(layer)
        } else {
            let mut current_layer = layer;
            // Iterate over effects
            for effect in effects {
                let effect_type = effect.effect_type.as_str();
                let gpu_context = self.renderer.get_gpu_context();

                let mut params = effect.properties.clone();
                params.insert(
                    "u_time".to_string(),
                    crate::model::property::PropertyValue::Number(ordered_float::OrderedFloat(
                        current_time,
                    )),
                );

                // Use the PluginManager to apply the effect
                current_layer = measure_debug(format!("Apply effect '{}'", effect_type), || {
                    self.plugin_manager.apply_effect(
                        effect_type,
                        &current_layer,
                        &params,
                        gpu_context,
                    )
                })?;
            }
            Ok(current_layer)
        }
    }

    /// Helper to load, apply effects, and draw a media surface (video or image).
    fn render_media_surface(
        &mut self,
        request: &LoadRequest,
        file_path: &str,
        transform: &Transform,
        effects: &[crate::model::frame::effect::ImageEffect],
        context: &RenderContext,
        current_time: f64,
    ) -> Result<(), LibraryError> {
        let image = measure_debug(format!("Load {}", file_path), || {
            self.plugin_manager
                .load_resource(request, &self.cache_manager)
                .map(|r| r.image)
        })?;

        let final_image = self.apply_effects(RenderOutput::Image(image), effects, current_time)?;

        let render_transform = context.transform(transform);

        measure_debug(format!("Draw {}", file_path), || {
            self.renderer.draw_layer_affine_with_blend(
                &final_image,
                &render_transform,
                transform.opacity,
                crate::model::BlendMode::Normal,
            )
        })?;

        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RenderContext {
    logical_to_target: Affine2D,
    render_scale: f64,
    target_width: u32,
    target_height: u32,
}

impl RenderContext {
    fn root(frame: &FrameInfo) -> Self {
        let render_scale = frame.render_scale.into_inner().max(f64::EPSILON);
        let (region_x, region_y, logical_width, logical_height) = frame.region.as_ref().map_or(
            (0.0, 0.0, frame.width as f64, frame.height as f64),
            |region| (region.x, region.y, region.width, region.height),
        );
        let logical_to_target = Affine2D::scale(render_scale, render_scale)
            .compose(Affine2D::translate(-region_x, -region_y));
        Self {
            logical_to_target,
            render_scale,
            target_width: scaled_dimension(logical_width, render_scale),
            target_height: scaled_dimension(logical_height, render_scale),
        }
    }

    fn composition(render_scale: f64, target_width: u32, target_height: u32) -> Self {
        Self {
            logical_to_target: Affine2D::scale(render_scale, render_scale),
            render_scale,
            target_width,
            target_height,
        }
    }

    fn with_transform(self, transform: &Transform) -> Self {
        Self {
            logical_to_target: self.logical_to_target.compose(Affine2D::from(transform)),
            ..self
        }
    }

    fn transform(self, transform: &Transform) -> Affine2D {
        self.logical_to_target.compose(Affine2D::from(transform))
    }
}

fn scaled_dimension(logical: f64, scale: f64) -> u32 {
    (logical * scale).round().clamp(1.0, u32::MAX as f64) as u32
}

fn transparent_color() -> crate::model::frame::color::Color {
    crate::model::frame::color::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn group_requires_isolation(group: &FrameGroup) -> bool {
    group.kind == FrameGroupKind::Merge
        || !group.effects.is_empty()
        || group.blend_mode != crate::model::BlendMode::Normal
        || (group.transform.opacity - 1.0).abs() > f64::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheManager;
    use crate::core::framing::FrameEvaluator;
    use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
    use crate::model::frame::color::Color;
    use crate::model::frame::frame::Region;
    use crate::model::project::{
        Composition, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
        PortAddress, PortOwner,
    };
    use crate::model::property::{Property, PropertyValue};
    use crate::model::{BlendMode, Clip, Node, Project, Track};
    use crate::plugin::{EffectPlugin, Plugin};
    use crate::rendering::skia_renderer::SkiaRenderer;
    use ordered_float::OrderedFloat;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingEffect {
        calls: Arc<AtomicUsize>,
    }

    struct TexturePathRenderer {
        saw_texture_layer: bool,
    }

    impl Renderer for TexturePathRenderer {
        fn draw_layer_affine_with_blend(
            &mut self,
            layer: &RenderOutput,
            _transform: &Affine2D,
            _opacity: f64,
            _blend_mode: BlendMode,
        ) -> Result<(), LibraryError> {
            self.saw_texture_layer |= matches!(layer, RenderOutput::Texture(_));
            Ok(())
        }

        fn begin_group(
            &mut self,
            _width: u32,
            _height: u32,
            _background_color: &Color,
        ) -> Result<(), LibraryError> {
            Ok(())
        }

        fn end_group(&mut self) -> Result<RenderOutput, LibraryError> {
            Ok(RenderOutput::Image(crate::model::frame::Image::new(
                1,
                1,
                vec![0, 0, 0, 0],
            )))
        }

        fn rasterize_text_layer(
            &mut self,
            _request: TextRasterRequest<'_>,
        ) -> Result<RenderOutput, LibraryError> {
            Err(LibraryError::Render("unexpected text".into()))
        }

        fn rasterize_shape_layer(
            &mut self,
            _request: ShapeRasterRequest<'_>,
        ) -> Result<RenderOutput, LibraryError> {
            Ok(RenderOutput::Texture(
                crate::rendering::renderer::TextureInfo {
                    texture_id: 7,
                    width: 1,
                    height: 1,
                },
            ))
        }

        fn rasterize_sksl_layer(
            &mut self,
            _shader_code: &str,
            _resolution: (f32, f32),
            _time: f32,
            _transform: &Affine2D,
        ) -> Result<RenderOutput, LibraryError> {
            Err(LibraryError::Render("unexpected SkSL".into()))
        }

        fn read_surface(
            &mut self,
            _output: &RenderOutput,
        ) -> Result<crate::model::frame::Image, LibraryError> {
            Err(LibraryError::Render("unexpected readback".into()))
        }

        fn finalize(&mut self) -> Result<RenderOutput, LibraryError> {
            Ok(RenderOutput::Texture(
                crate::rendering::renderer::TextureInfo {
                    texture_id: 99,
                    width: 1,
                    height: 1,
                },
            ))
        }

        fn clear(&mut self) -> Result<(), LibraryError> {
            Ok(())
        }
    }

    impl Plugin for CountingEffect {
        fn id(&self) -> &'static str {
            "counting_track_effect"
        }

        fn name(&self) -> String {
            "Counting Track Effect".into()
        }

        fn category(&self) -> String {
            "Test".into()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 0, 0)
        }
    }

    impl EffectPlugin for CountingEffect {
        fn apply(
            &self,
            input: &RenderOutput,
            _params: &HashMap<String, PropertyValue>,
            _gpu_context: Option<&mut crate::rendering::skia_utils::GpuContext>,
        ) -> Result<RenderOutput, LibraryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(input.clone())
        }

        fn properties(&self) -> Vec<crate::model::property::PropertyDefinition> {
            Vec::new()
        }
    }

    fn add_solid(
        project: &mut Project,
        track_id: uuid::Uuid,
        color: Color,
    ) -> (uuid::Uuid, uuid::Uuid) {
        let clip = Clip::new("solid clip", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        let node = test_generator_node("solid", GeneratorNodeRequest::Solid { color });
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
        (clip_id, node_id)
    }

    #[test]
    fn real_render_path_composites_tracks_in_order_with_opacity_blend_and_effects() {
        let effect_calls = Arc::new(AtomicUsize::new(0));
        let plugin_manager = Arc::new(PluginManager::default());
        plugin_manager.register_effect(Arc::new(CountingEffect {
            calls: Arc::clone(&effect_calls),
        }));
        let mut project = Project::new("track render test");
        let (mut composition, first_track) = Composition::new("main", 8, 8, 30.0, 1.0);
        composition.background_color = Color::black();
        let composition_id = composition.id;
        let first_track_id = first_track.id;
        assert!(
            project.add_track(first_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );

        let mut second_track = Track::new("second");
        let second_track_id = second_track.id;
        second_track.properties.set(
            "opacity".into(),
            Property::constant(PropertyValue::Number(OrderedFloat(50.0))),
        );
        assert!(
            project.add_track(second_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project
            .attach_track_to_composition(composition_id, second_track_id)
            .unwrap();
        let second_track_edge_id = project
            .connections
            .iter()
            .find(|connection| connection.from.owner == PortOwner::Track(second_track_id))
            .expect("Track insertion must create a structural Merge edge")
            .id;
        project
            .set_connection_blend_mode(second_track_edge_id, BlendMode::LinearDodge)
            .unwrap();

        let _ = add_solid(
            &mut project,
            first_track_id,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        );
        let (_second_clip_id, _) = add_solid(
            &mut project,
            second_track_id,
            Color {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
        );
        let effect = plugin_manager
            .create_effect_operation_node("counting_track_effect")
            .unwrap();
        let effect_id = effect.id;
        project.add_node(effect);
        project
            .attach_node_to_container(NodeContainer::Track(second_track_id), effect_id)
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(
                    PortOwner::Node(
                        project
                            .get_track(second_track_id)
                            .unwrap()
                            .structural_merge_node_id,
                    ),
                    IMAGE_OUTPUT_PORT,
                ),
                PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
            )
            .unwrap();
        project
            .set_output_node(NodeContainer::Track(second_track_id), Some(effect_id))
            .unwrap();
        let frame = FrameEvaluator::new(
            &project,
            &project.compositions[0],
            plugin_manager.get_property_evaluators(),
            plugin_manager.as_ref(),
        )
        .evaluate(
            0,
            0.5,
            Some(Region {
                x: 2.0,
                y: 2.0,
                width: 4.0,
                height: 4.0,
            }),
        )
        .unwrap();

        let renderer = SkiaRenderer::new(2, 2, Color::black(), false, None, None).unwrap();
        let mut service = RenderService::new(
            renderer,
            Arc::clone(&plugin_manager),
            Arc::new(CacheManager::new()),
        );
        let RenderOutput::Image(image) = service.render_from_frame_info(&frame).unwrap() else {
            panic!("CPU renderer must return an Image");
        };

        assert_eq!((image.width, image.height), (2, 2));
        let pixel = &image.data[0..4];
        assert!(
            pixel[0] >= 250,
            "bottom red Track must render first: {pixel:?}"
        );
        assert!(
            (120..=135).contains(&pixel[1]),
            "top green Track must be added once at 50% opacity: {pixel:?}"
        );
        assert!(pixel[2] <= 2, "unexpected blue contribution: {pixel:?}");
        assert_eq!(pixel[3], 255);
        assert_eq!(effect_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn composition_instance_is_a_spatially_neutral_single_image_output() {
        let mut project = Project::new("composition output test");
        let (mut parent, parent_track) = Composition::new("parent", 1, 1, 30.0, 1.0);
        parent.background_color = Color::black();
        let parent_track_id = parent_track.id;
        assert!(
            project.add_track(parent_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(parent).is_ok(),
            "container structural Merge insertion must succeed"
        );

        let (mut nested, nested_track) = Composition::new("nested", 1, 1, 30.0, 1.0);
        nested.background_color = Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        };
        let nested_id = nested.id;
        let nested_track_id = nested_track.id;
        assert!(
            project.add_track(nested_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(nested).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let _ = add_solid(
            &mut project,
            nested_track_id,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        );

        let clip = Clip::new("nested instance clip", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project
            .attach_clip_to_track(parent_track_id, clip_id)
            .unwrap();
        let instance = Node::new_composition_instance(
            "nested instance",
            crate::model::CompositionInstanceContent {
                composition_id: nested_id,
            },
        );
        let instance_id = instance.id;
        project.add_node(instance);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), instance_id)
            .unwrap();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(instance_id))
            .unwrap();

        let plugin_manager = Arc::new(PluginManager::default());
        let frame = FrameEvaluator::new(
            &project,
            &project.compositions[0],
            plugin_manager.get_property_evaluators(),
            plugin_manager.as_ref(),
        )
        .evaluate(0, 1.0, None)
        .unwrap();
        let renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
        let mut service =
            RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));
        let RenderOutput::Image(image) = service.render_from_frame_info(&frame).unwrap() else {
            panic!("CPU renderer must return an Image");
        };

        assert_eq!(&image.data[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn hierarchical_rendering_preserves_texture_layers_and_root_texture_output() {
        let mut project = Project::new("texture path test");
        let (composition, track) = Composition::new("main", 1, 1, 30.0, 1.0);
        let track_id = track.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let _ = add_solid(&mut project, track_id, Color::white());

        let plugin_manager = Arc::new(PluginManager::default());
        let frame = FrameEvaluator::new(
            &project,
            &project.compositions[0],
            plugin_manager.get_property_evaluators(),
            plugin_manager.as_ref(),
        )
        .evaluate(0, 1.0, None)
        .unwrap();
        let renderer = TexturePathRenderer {
            saw_texture_layer: false,
        };
        let mut service =
            RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));

        let output = service.render_from_frame_info(&frame).unwrap();
        assert!(service.renderer.saw_texture_layer);
        assert!(matches!(
            output,
            RenderOutput::Texture(crate::rendering::renderer::TextureInfo { texture_id: 99, .. })
        ));
    }

    #[test]
    fn merge_connection_order_changes_the_rendered_pixel() {
        let mut project = Project::new("merge pixel test");
        let (mut composition, track) = Composition::new("main", 1, 1, 30.0, 1.0);
        composition.background_color = Color::black();
        let track_id = track.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );

        let clip = Clip::new("merge clip", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let red = test_generator_node(
            "red",
            GeneratorNodeRequest::Solid {
                color: Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            },
        );
        let red_id = red.id;
        project.add_node(red);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), red_id)
            .unwrap();

        let mut green = test_generator_node(
            "green",
            GeneratorNodeRequest::Solid {
                color: Color {
                    r: 0,
                    g: 255,
                    b: 0,
                    a: 255,
                },
            },
        );
        green.blend_mode = BlendMode::Multiply;
        let green_id = green.id;
        project.add_node(green);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), green_id)
            .unwrap();

        let merge = Node::new_merge("merge");
        let merge_id = merge.id;
        project.add_node(merge);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), merge_id)
            .unwrap();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
            .unwrap();

        let target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
        let red_connection = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(red_id), IMAGE_OUTPUT_PORT),
                target.clone(),
            )
            .unwrap();
        let green_connection = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(green_id), IMAGE_OUTPUT_PORT),
                target,
            )
            .unwrap();
        project
            .set_connection_blend_mode(green_connection, BlendMode::LinearDodge)
            .unwrap();

        let plugin_manager = Arc::new(PluginManager::default());
        let render_pixel = |project: &Project| {
            let frame = FrameEvaluator::new(
                project,
                &project.compositions[0],
                plugin_manager.get_property_evaluators(),
                plugin_manager.as_ref(),
            )
            .evaluate(0, 1.0, None)
            .unwrap();
            let renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
            let mut service = RenderService::new(
                renderer,
                Arc::clone(&plugin_manager),
                Arc::new(CacheManager::new()),
            );
            let RenderOutput::Image(image) = service.render_from_frame_info(&frame).unwrap() else {
                panic!("CPU renderer must return an Image");
            };
            image.data[0..4].to_vec()
        };

        let additive = render_pixel(&project);
        assert!(additive[0] >= 250, "red contribution missing: {additive:?}");
        assert!(
            additive[1] >= 250,
            "green Add contribution missing: {additive:?}"
        );
        assert!(
            additive[2] <= 2,
            "unexpected blue contribution: {additive:?}"
        );

        project.reorder_connection(green_connection, 0).unwrap();
        let reordered = render_pixel(&project);
        assert!(reordered[0] >= 250, "red top image missing: {reordered:?}");
        assert!(
            reordered[1] <= 2,
            "reorder did not change output: {reordered:?}"
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|connection| connection.id == red_connection)
                .unwrap()
                .order,
            1
        );
    }
}
