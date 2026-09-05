use super::*;
use crate::cache::CacheManager;
use crate::core::framing::FrameEvaluator;
use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
use crate::model::authoring::{InstancePath, ModuleInstanceId, ModuleOutputId, TimelineId};
use crate::model::frame::color::Color;
use crate::model::frame::frame::Region;
use crate::model::frame::particle::{
    ParticleSceneFrame, ParticleSceneParameters, SceneInvocationKey,
};
use crate::model::project::{
    Composition, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    PortAddress, PortOwner,
};
use crate::model::property::{Property, PropertyValue};
use crate::model::{
    BlendMode, COLOR_ALPHA_PORT, COLOR_BLUE_PORT, COLOR_GREEN_PORT, COLOR_RED_PORT,
    COLOR_SPACE_PORT, COLOR_VALUE_PORT, Clip, ColorContent, Node, Project, Track,
};
use crate::plugin::{EffectPlugin, Plugin};
use crate::rendering::skia_renderer::SkiaRenderer;
use ordered_float::OrderedFloat;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn output_times_map_to_source_frames_with_interval_semantics() {
    let upsampled: Vec<u64> = (0..120)
        .map(|output_frame| composition_frame_at_time(output_frame as f64 / 60.0, 30.0).unwrap())
        .collect();
    assert_eq!(&upsampled[..6], &[0, 0, 1, 1, 2, 2]);
    assert_eq!(&upsampled[114..], &[57, 57, 58, 58, 59, 59]);
    assert_eq!(upsampled.iter().copied().max(), Some(59));

    let downsampled: Vec<u64> = (0..48)
        .map(|output_frame| composition_frame_at_time(output_frame as f64 / 24.0, 60.0).unwrap())
        .collect();
    assert_eq!(&downsampled[..4], &[0, 2, 5, 7]);
    assert_eq!(downsampled.last(), Some(&117));

    assert_eq!(composition_frame_at_time(59.5 / 30.0, 30.0).unwrap(), 59);
    assert_eq!(composition_frame_at_time(5.0 / 30.0, 30.0).unwrap(), 5);
}

#[test]
fn source_frame_mapping_rejects_invalid_time_and_rate() {
    for time in [f64::NAN, f64::INFINITY, -0.01] {
        assert!(composition_frame_at_time(time, 30.0).is_err());
    }
    for fps in [f64::NAN, f64::INFINITY, 0.0, -30.0] {
        assert!(composition_frame_at_time(0.0, fps).is_err());
    }
}

struct CountingEffect {
    calls: Arc<AtomicUsize>,
}

struct TexturePathRenderer {
    saw_texture_layer: bool,
    native_group_composites: usize,
    direct_particle_draws: usize,
    particle_rasterizations: usize,
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

    fn end_group_and_draw(
        &mut self,
        _transform: &Affine2D,
        _opacity: f64,
        _blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        self.native_group_composites += 1;
        Ok(())
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
        _request: crate::rendering::renderer::SkSLRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        Err(LibraryError::Render("unexpected SkSL".into()))
    }

    fn rasterize_particle_layer(
        &mut self,
        _request: ParticleRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        self.particle_rasterizations += 1;
        Ok(RenderOutput::Image(crate::model::frame::Image::new(
            1,
            1,
            vec![0, 0, 0, 0],
        )))
    }

    fn draw_particle_layer(
        &mut self,
        _request: ParticleRasterRequest<'_>,
        _opacity: f64,
        _blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        self.direct_particle_draws += 1;
        Ok(())
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
    let mut service = RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));
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
        native_group_composites: 0,
        direct_particle_draws: 0,
        particle_rasterizations: 0,
    };
    let mut service = RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));

    let output = service.render_from_frame_info(&frame).unwrap();
    assert!(service.renderer.saw_texture_layer);
    assert!(
        service.renderer.native_group_composites > 0,
        "no-effect Composition/Merge groups must use the backend-native combined boundary"
    );
    assert!(matches!(
        output,
        RenderOutput::Texture(crate::rendering::renderer::TextureInfo { texture_id: 99, .. })
    ));
}

#[test]
fn particle_without_effects_uses_the_backend_native_draw_boundary() {
    let scene = ParticleSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: InstancePath::root(TimelineId::new()),
            module_instance_id: ModuleInstanceId::new(),
            state_slot_id: uuid::Uuid::new_v4(),
            output_id: ModuleOutputId::new(),
        },
        random_stream_id: uuid::Uuid::new_v4(),
        executable_hash: [1; 32],
        target_step: 1,
        logical_width: 1,
        logical_height: 1,
        parameters: ParticleSceneParameters {
            capacity: 1,
            emission_rate: OrderedFloat(1.0),
            lifetime_seconds: OrderedFloat(1.0),
            seed: 1,
            velocity_min: crate::model::property::Vec3 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(0.0),
                z: OrderedFloat(0.0),
            },
            velocity_max: crate::model::property::Vec3 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(0.0),
                z: OrderedFloat(0.0),
            },
            gravity: crate::model::property::Vec3 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(0.0),
                z: OrderedFloat(0.0),
            },
            drag: OrderedFloat(0.0),
            size_min: OrderedFloat(1.0),
            size_max: OrderedFloat(1.0),
            color: Color::white(),
        },
    };
    let frame = FrameInfo {
        width: 1,
        height: 1,
        background_color: Color::black(),
        color_profile: String::new(),
        render_scale: OrderedFloat(1.0),
        now_time: OrderedFloat(0.0),
        region: None,
        items: vec![FrameItem::Object(FrameObject {
            source_node_id: uuid::Uuid::new_v4(),
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: None,
            content: FrameContent::ParticleScene {
                scene,
                effects: Vec::new(),
                transform: Transform::default(),
            },
        })],
    };
    let renderer = TexturePathRenderer {
        saw_texture_layer: false,
        native_group_composites: 0,
        direct_particle_draws: 0,
        particle_rasterizations: 0,
    };
    let mut service = RenderService::new(
        renderer,
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );

    service
        .render_from_frame_info(&frame)
        .expect("backend-native Particle draw");
    assert_eq!(service.renderer.direct_particle_draws, 1);
    assert_eq!(service.renderer.particle_rasterizations, 0);
}

#[test]
fn managed_export_uses_the_same_shape_graph_with_a_linear_working_surface() {
    let mut project = Project::new("managed export linear shape test");
    let (mut composition, track) = Composition::new("main", 1, 1, 30.0, 1.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    project
        .add_track(track)
        .expect("insert export fixture Track");
    project
        .add_composition(composition)
        .expect("insert export fixture Composition");
    let _ = add_solid(
        &mut project,
        track_id,
        Color {
            r: 255,
            g: 255,
            b: 255,
            a: 128,
        },
    );

    let plugin_manager = Arc::new(PluginManager::default());
    let project_model = ProjectModel::new(Arc::new(project), 0).expect("valid export fixture");
    let renderer =
        SkiaRenderer::new(1, 1, Color::black(), false, None, None).expect("CPU export renderer");
    let mut service = RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));

    let frame = service
        .get_frame(&project_model, 0.0)
        .expect("evaluate half-transparent white fixture");
    let RenderOutput::Image(legacy_image) = service
        .render_from_frame_info(&frame)
        .expect("explicit unmanaged ABI demonstrates the legacy result")
    else {
        panic!("CPU compatibility renderer must produce owned pixels");
    };
    assert_eq!(
        legacy_image.data,
        [128, 128, 128, 255],
        "encoded-sRGB compositing is the gamma-wrong result that export must refuse"
    );

    let export = service
        .render_export_frame(&project_model, 0.0)
        .expect("Project shape graph must render through the scene-linear Skia surface");
    assert_eq!(
        export.image().data,
        [188, 188, 188, 255],
        "half-white over black must composite in linear light and terminal exactly once"
    );
}

#[test]
fn managed_silhouette_converts_wired_gray_through_project_working_space() {
    let plugin_manager = Arc::new(PluginManager::default());
    let sksl_directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/plugins/sksl");
    plugin_manager
        .load_sksl_plugins_from_directory(sksl_directory)
        .expect("load bundled SkSL effects");

    let mut project = Project::new("managed silhouette color test");
    let (mut composition, track) = Composition::new("main", 1, 1, 30.0, 1.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    project.add_track(track).expect("insert fixture Track");
    project
        .add_composition(composition)
        .expect("insert fixture Composition");
    let (clip_id, source_id) = add_solid(&mut project, track_id, Color::white());

    let silhouette = plugin_manager
        .create_effect_operation_node("silhouette")
        .expect("bundled silhouette operation");
    assert!(matches!(
        silhouette
            .properties()
            .get("color")
            .and_then(|property| property.value()),
        Some(PropertyValue::ColorValue(_))
    ));
    let silhouette_id = silhouette.id;
    project.add_node(silhouette);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), silhouette_id)
        .unwrap();

    let mut authored_color = Node::new_color("Authored gray", ColorContent::Compose);
    for (name, value) in [
        (
            COLOR_SPACE_PORT,
            PropertyValue::String(crate::color_management::encoded_srgb_space_id().to_string()),
        ),
        (
            COLOR_RED_PORT,
            PropertyValue::Number(OrderedFloat(128.0 / 255.0)),
        ),
        (
            COLOR_GREEN_PORT,
            PropertyValue::Number(OrderedFloat(128.0 / 255.0)),
        ),
        (
            COLOR_BLUE_PORT,
            PropertyValue::Number(OrderedFloat(128.0 / 255.0)),
        ),
        (COLOR_ALPHA_PORT, PropertyValue::Number(OrderedFloat(1.0))),
    ] {
        authored_color
            .set_property(name.to_string(), Property::constant(value))
            .expect("set authored graph color component");
    }
    let authored_color_id = authored_color.id;
    project.add_node(authored_color);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), authored_color_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(authored_color_id), COLOR_VALUE_PORT),
            PortAddress::new(
                PortOwner::Node(silhouette_id),
                crate::plugin::property_port_key("color"),
            ),
        )
        .expect("connect typed graph color to silhouette");
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(silhouette_id), IMAGE_INPUT_PORT),
        )
        .unwrap();
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(silhouette_id))
        .unwrap();

    let project_model = ProjectModel::new(Arc::new(project), 0).expect("valid fixture Project");
    let renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    let mut service = RenderService::new(
        renderer,
        Arc::clone(&plugin_manager),
        Arc::new(CacheManager::new()),
    );
    let export = service
        .render_export_frame(&project_model, 0.0)
        .expect("silhouette should render through Project linear RGBAF32");
    assert_eq!(
        export.image().data,
        [128, 128, 128, 255],
        "authored sRGB 128 must become about 0.21586 in linear-sRGB and transform back once; treating 128/255 as linear would produce about 188"
    );
}

#[test]
fn managed_export_rejects_an_untyped_effect_before_plugin_execution() {
    let effect_calls = Arc::new(AtomicUsize::new(0));
    let plugin_manager = Arc::new(PluginManager::default());
    plugin_manager.register_effect(Arc::new(CountingEffect {
        calls: Arc::clone(&effect_calls),
    }));
    let mut project = Project::new("managed effect contract test");
    let (mut composition, track) = Composition::new("main", 1, 1, 30.0, 1.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    project.add_track(track).expect("insert fixture Track");
    project
        .add_composition(composition)
        .expect("insert fixture Composition");
    let _ = add_solid(&mut project, track_id, Color::white());

    let effect = plugin_manager
        .create_effect_operation_node("counting_track_effect")
        .expect("test effect operation");
    let effect_id = effect.id;
    project.add_node(effect);
    project
        .attach_node_to_container(NodeContainer::Track(track_id), effect_id)
        .unwrap();
    let merge_id = project
        .get_track(track_id)
        .expect("fixture Track")
        .structural_merge_node_id;
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(merge_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
        )
        .unwrap();
    project
        .set_output_node(NodeContainer::Track(track_id), Some(effect_id))
        .unwrap();

    let project_model = ProjectModel::new(Arc::new(project), 0).expect("valid fixture Project");
    let renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    let mut service = RenderService::new(renderer, plugin_manager, Arc::new(CacheManager::new()));
    let error = service
        .render_export_frame(&project_model, 0.0)
        .expect_err("legacy-only effect must fail before ExportFrame construction");
    assert!(error.to_string().contains("unmanaged encoded-sRGBA8"));
    assert_eq!(
        effect_calls.load(Ordering::SeqCst),
        0,
        "domain admission must reject the effect before its implementation runs"
    );
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
