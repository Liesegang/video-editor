use std::collections::BTreeSet;
use std::sync::Arc;

use library::SkiaRenderer;
use library::core::cache::CacheManager;
use library::core::ensemble::BackplateTarget;
use library::core::ensemble::decorators::BackplateShape;
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::{DecoratorConfig, EffectorConfig};
use library::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use library::editor::{
    AuthoringPropertyOwner, AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate,
    RenderDestination, RenderService, TextEnsembleOperationKind, TimelineEditorService,
    build_authoring_e2e_fixture,
};
use library::model::authoring::{
    AuthoringProject, MediaTime, ProjectDocument, RationalRate, SourceRef, TimelineInterval,
    TimelineItemId,
};
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::project::{BACKGROUND_SHAPE_INPUT_PORT, PortDataType, PortDefinition};
use library::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use library::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, DecoratorPlugin, EFFECTOR_APPLY_OPERATION,
    EFFECTOR_CATEGORY, EvaluatedOperation, OperationDescriptor, OperationDescriptorError, Plugin,
    PluginCategory, PluginManager,
};
use library::rendering::renderer::RenderOutput;

const INLINE_DECORATOR_ID: &str = "inline_test_backplate";

struct InlineBackplateDecorator;

impl Plugin for InlineBackplateDecorator {
    fn id(&self) -> &'static str {
        INLINE_DECORATOR_ID
    }

    fn name(&self) -> String {
        "Inline test backplate".to_string()
    }

    fn category(&self) -> String {
        "Test".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl DecoratorPlugin for InlineBackplateDecorator {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![PropertyDefinition::new(
            "padding",
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix: "px".to_string(),
                min_hard_limit: false,
                max_hard_limit: false,
            },
            "Padding",
            PropertyValue::from(0.0),
        )]
    }

    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::decorator(self.id(), self.name(), self.properties())
    }

    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: uuid::Uuid,
    ) -> Option<DecoratorConfig> {
        let padding = context.number("padding").unwrap_or(0.0) as f32;
        Some(DecoratorConfig::LegacyBackplate {
            target: BackplateTarget::Block,
            shape: BackplateShape::RoundedRect,
            color: Color::black(),
            padding: (padding, padding, padding, padding),
            corner_radius: 4.0,
        })
    }

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Decorator
    }
}

fn plugins_with_inline_decorator() -> PluginManager {
    let plugins = PluginManager::default();
    plugins.register_decorator_plugin(Arc::new(InlineBackplateDecorator));
    plugins
}

fn seconds(value: i64) -> MediaTime {
    MediaTime::from_whole_seconds(value)
}

fn text_service() -> Result<(TimelineEditorService, TimelineItemId), String> {
    let project = AuthoringProject::new(
        "Text Ensemble",
        320,
        180,
        RationalRate::new(30, 1)?,
        seconds(4),
    )?;
    let service = TimelineEditorService::new(project).map_err(|error| error.to_string())?;
    let project = service.snapshot().map_err(|error| error.to_string())?;
    let track_id = project
        .timelines
        .get(&project.root_timeline_id)
        .and_then(|timeline| timeline.track_order.first())
        .copied()
        .ok_or_else(|| "Text Ensemble fixture has no default Track".to_string())?;
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Animated text".to_string(),
            SourceRef::Text {
                text: "Ensemble".to_string(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(seconds(0), seconds(4))?,
            0,
        )
        .map_err(|error| error.to_string())?;
    Ok((service, item_id))
}

fn text_operations(
    project: &library::model::authoring::AuthoringProject,
    item_id: TimelineItemId,
) -> Result<&[library::model::authoring::TextEnsembleOperation], &'static str> {
    let item = project
        .items
        .get(&item_id)
        .ok_or("Text Ensemble fixture item is missing")?;
    let SourceRef::Text {
        ensemble_operations,
        ..
    } = &item.source
    else {
        return Err("Text Ensemble fixture item must remain Text");
    };
    Ok(ensemble_operations)
}

#[test]
fn descriptor_operations_edit_reorder_remove_and_round_trip() {
    let plugins = plugins_with_inline_decorator();
    let (service, item_id) = text_service().expect("valid Text Ensemble fixture");
    let (effector_id, _) = service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "transform",
        )
        .unwrap();
    let (opacity_id, _) = service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "opacity",
        )
        .unwrap();
    let (decorator_id, _) = service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Decorator,
            INLINE_DECORATOR_ID,
        )
        .unwrap();

    service
        .set_text_ensemble_property(
            &plugins,
            item_id,
            effector_id,
            "tx",
            seconds(0),
            PropertyValue::from(48.0),
        )
        .unwrap();
    service
        .reorder_text_ensemble_operation(item_id, opacity_id, 0)
        .unwrap();

    let project = service.snapshot().unwrap();
    let operations = text_operations(&project, item_id).expect("Text operations");
    assert_eq!(
        operations.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![opacity_id, effector_id, decorator_id]
    );
    for operation in operations {
        let descriptor = plugins
            .operation_descriptor(
                &operation.operation.category,
                &operation.operation.component_id,
                &operation.operation.operation,
            )
            .unwrap();
        let authored = operation
            .properties
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<BTreeSet<_>>();
        let declared = descriptor
            .properties()
            .iter()
            .map(|definition| definition.name())
            .collect::<BTreeSet<_>>();
        assert_eq!(authored, declared, "controls must come from the descriptor");
    }
    assert_eq!(operations[0].operation.category, EFFECTOR_CATEGORY);
    assert_eq!(operations[0].operation.operation, EFFECTOR_APPLY_OPERATION);
    assert_eq!(operations[1].operation.category, EFFECTOR_CATEGORY);
    assert_eq!(operations[1].operation.operation, EFFECTOR_APPLY_OPERATION);
    assert_eq!(operations[2].operation.category, DECORATOR_CATEGORY);
    assert_eq!(operations[2].operation.operation, DECORATOR_APPLY_OPERATION);
    assert_eq!(
        operations[1].properties.get("tx").unwrap().value(),
        Some(&PropertyValue::from(48.0))
    );

    let json = ProjectDocument::new(project.as_ref().clone())
        .to_json()
        .unwrap();
    let loaded = ProjectDocument::from_json(&json).unwrap();
    assert_eq!(loaded.project, project.as_ref().clone());
    let mut invalid_phase = ProjectDocument::new(project.as_ref().clone());
    text_operations_mut(&mut invalid_phase.project, item_id)
        .expect("Text operations")
        .swap(0, 2);
    assert!(
        invalid_phase
            .to_json()
            .unwrap_err()
            .contains("Effector after the Decorator phase")
    );

    service
        .remove_text_ensemble_operation(item_id, decorator_id)
        .unwrap();
    assert_eq!(
        text_operations(&service.snapshot().unwrap(), item_id)
            .expect("Text operations")
            .len(),
        2
    );
    service.undo().unwrap().expect("remove is one undo step");
    assert_eq!(
        text_operations(&service.snapshot().unwrap(), item_id)
            .expect("Text operations")
            .len(),
        3
    );
    service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Decorator,
            "backplate",
        )
        .expect("the production Backplate keeps its self-contained Text form");
}

#[test]
fn built_in_backplate_has_one_component_with_graph_and_self_contained_contracts() {
    let plugins = PluginManager::default();
    let graph = plugins
        .operation_descriptor(DECORATOR_CATEGORY, "backplate", DECORATOR_APPLY_OPERATION)
        .unwrap();
    assert!(graph.declared_ports().iter().any(|port| {
        port.key == BACKGROUND_SHAPE_INPUT_PORT
            && port.direction == library::model::project::PortDirection::Input
    }));
    let inline = plugins
        .text_ensemble_operation_descriptor(DECORATOR_CATEGORY, "backplate")
        .unwrap();
    assert!(
        !inline
            .declared_ports()
            .iter()
            .any(|port| port.key == BACKGROUND_SHAPE_INPUT_PORT)
    );
    assert_eq!(
        inline
            .properties()
            .iter()
            .map(|definition| definition.name())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["color", "padding", "radius", "shape", "target"])
    );

    let (service, item_id) = text_service().expect("valid Text Ensemble fixture");
    let (operation_id, _) = service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Decorator,
            "backplate",
        )
        .unwrap();
    for (key, value) in [
        ("target", PropertyValue::String("Line".to_string())),
        ("shape", PropertyValue::String("RoundRect".to_string())),
        ("padding", PropertyValue::from(12.0)),
        ("radius", PropertyValue::from(6.0)),
    ] {
        service
            .set_text_ensemble_property(&plugins, item_id, operation_id, key, seconds(0), value)
            .unwrap();
    }
    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame = evaluate_render_plan_frame(&project, &plan, &plugins, 0, 1.0, None).unwrap();
    assert!(matches!(
        find_text_ensemble(&frame.items)
            .expect("Text must reach frame IR")
            .decorator_configs
            .as_slice(),
        [DecoratorConfig::LegacyBackplate {
            target: BackplateTarget::Line,
            shape: BackplateShape::RoundedRect,
            padding: (12.0, 12.0, 12.0, 12.0),
            corner_radius: 6.0,
            ..
        }]
    ));
}

#[test]
fn text_ensemble_schema_is_strict_and_rejects_duplicate_operation_ids() {
    let plugins = PluginManager::default();
    let (service, item_id) = text_service().expect("valid Text Ensemble fixture");
    service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "transform",
        )
        .unwrap();
    let mut media_input_document = service.document().unwrap();
    text_operations_mut(&mut media_input_document.project, item_id).expect("Text operations")[0]
        .declared_ports
        .push(PortDefinition::input(
            BACKGROUND_SHAPE_INPUT_PORT,
            "Background",
            PortDataType::Shape,
        ));
    assert!(
        media_input_document
            .to_json()
            .unwrap_err()
            .contains("unsupported media inputs")
    );

    let mut document = service.document().unwrap();
    let operation =
        text_operations(&document.project, item_id).expect("Text operations")[0].clone();
    text_operations_mut(&mut document.project, item_id)
        .expect("Text operations")
        .push(operation);
    assert!(document.to_json().unwrap_err().contains("repeats or omits"));

    let valid = service.document().unwrap().to_json().unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&valid).unwrap();
    json["project"]["items"][item_id.to_string()]["source"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("ensemble_operations");
    assert!(
        ProjectDocument::from_json(&serde_json::to_string(&json).unwrap())
            .unwrap_err()
            .contains("ensemble_operations")
    );
}

fn text_operations_mut(
    project: &mut library::model::authoring::AuthoringProject,
    item_id: TimelineItemId,
) -> Result<&mut Vec<library::model::authoring::TextEnsembleOperation>, &'static str> {
    let item = project
        .items
        .get_mut(&item_id)
        .ok_or("Text Ensemble fixture item is missing")?;
    let SourceRef::Text {
        ensemble_operations,
        ..
    } = &mut item.source
    else {
        return Err("Text Ensemble fixture item must remain Text");
    };
    Ok(ensemble_operations)
}

#[test]
fn timeline_frame_uses_production_effector_and_decorator_evaluators() {
    let plugins = Arc::new(plugins_with_inline_decorator());
    let (service, item_id) = text_service().expect("valid Text Ensemble fixture");
    let baseline_project = service.snapshot().unwrap();
    let baseline_plan = RenderPlanCompiler::compile(&baseline_project).unwrap();
    let baseline_frame = evaluate_render_plan_frame(
        &baseline_project,
        &baseline_plan,
        plugins.as_ref(),
        0,
        1.0,
        None,
    )
    .unwrap();
    let (effector_id, _) = service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            item_id,
            TextEnsembleOperationKind::Effector,
            "transform",
        )
        .unwrap();
    let (decorator_id, _) = service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            item_id,
            TextEnsembleOperationKind::Decorator,
            INLINE_DECORATOR_ID,
        )
        .unwrap();
    service
        .set_text_ensemble_property(
            plugins.as_ref(),
            item_id,
            effector_id,
            "tx",
            seconds(0),
            PropertyValue::from(36.0),
        )
        .unwrap();
    service
        .set_text_ensemble_property(
            plugins.as_ref(),
            item_id,
            decorator_id,
            "padding",
            seconds(0),
            PropertyValue::from(12.0),
        )
        .unwrap();

    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame =
        evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None).unwrap();
    let ensemble = find_text_ensemble(&frame.items).expect("Text must reach frame IR");
    assert_eq!(ensemble.effector_configs.len(), 1);
    assert_eq!(ensemble.decorator_configs.len(), 1);
    assert!(matches!(
        ensemble.effector_configs[0],
        EffectorConfig::Transform {
            translate: (36.0, 0.0),
            ..
        }
    ));
    assert!(matches!(
        ensemble.decorator_configs[0],
        DecoratorConfig::LegacyBackplate {
            padding: (12.0, 12.0, 12.0, 12.0),
            ..
        }
    ));

    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(
        320,
        180,
        Color::black(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )
    .unwrap();
    let mut renderer = RenderService::new(renderer, Arc::clone(&plugins), cache);
    for destination in [RenderDestination::Preview, RenderDestination::Export] {
        let RenderOutput::Image(baseline) = renderer
            .render_authoring_frame(&baseline_project, &baseline_frame, destination)
            .unwrap()
        else {
            panic!("authoring render must terminate to an Image");
        };
        let RenderOutput::Image(ensemble) = renderer
            .render_authoring_frame(&project, &frame, destination)
            .unwrap()
        else {
            panic!("authoring render must terminate to an Image");
        };
        assert_ne!(
            ensemble.data, baseline.data,
            "{destination:?} must rasterize the evaluated Ensemble"
        );
    }
}

#[test]
fn transform_controls_reach_frame_ir_with_independent_axes_rotation_and_target() {
    let plugins = Arc::new(PluginManager::default());
    let (service, item_id) = text_service().expect("valid Text Ensemble fixture");
    service
        .set_text(item_id, "AB\nCD".to_string())
        .expect("multi-line Text");
    let (operation_id, _) = service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            item_id,
            TextEnsembleOperationKind::Effector,
            "transform",
        )
        .unwrap();
    for (key, value) in [
        ("tx", PropertyValue::from(37.0)),
        ("ty", PropertyValue::from(-19.0)),
        ("scale_x", PropertyValue::from(2.5)),
        ("scale_y", PropertyValue::from(0.4)),
        ("rotation", PropertyValue::from(31.0)),
        ("target", PropertyValue::String("Line".to_string())),
    ] {
        service
            .set_text_ensemble_property(
                plugins.as_ref(),
                item_id,
                operation_id,
                key,
                seconds(0),
                value,
            )
            .unwrap();
    }

    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame =
        evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None).unwrap();
    assert!(matches!(
        find_text_ensemble(&frame.items)
            .expect("Text must reach frame IR")
            .effector_configs
            .as_slice(),
        [EffectorConfig::Transform {
            translate: (37.0, -19.0),
            rotate: 31.0,
            scale: (2.5, 0.4),
            target: EffectorTarget::Line,
        }]
    ));

    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(
        320,
        180,
        Color::black(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )
    .unwrap();
    let mut renderer = RenderService::new(renderer, Arc::clone(&plugins), cache);
    let mut rendered_targets = Vec::new();
    for (authored_target, expected_target) in [
        ("Block", EffectorTarget::Block),
        ("Line", EffectorTarget::Line),
        ("Char", EffectorTarget::Char),
    ] {
        service
            .set_text_ensemble_property(
                plugins.as_ref(),
                item_id,
                operation_id,
                "target",
                seconds(0),
                PropertyValue::String(authored_target.to_string()),
            )
            .unwrap();
        let project = service.snapshot().unwrap();
        let plan = RenderPlanCompiler::compile(&project).unwrap();
        let frame =
            evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 0, 1.0, None).unwrap();
        assert!(matches!(
            find_text_ensemble(&frame.items)
                .expect("Text must reach RuntimeShape")
                .effector_configs
                .as_slice(),
            [EffectorConfig::Transform { target, .. }] if *target == expected_target
        ));
        let RenderOutput::Image(image) = renderer
            .render_authoring_frame(&project, &frame, RenderDestination::Preview)
            .unwrap()
        else {
            panic!("Text Ensemble must rasterize to an Image");
        };
        rendered_targets.push(image.data);
    }
    assert_ne!(rendered_targets[0], rendered_targets[1]);
    assert_ne!(rendered_targets[1], rendered_targets[2]);
    assert_ne!(rendered_targets[0], rendered_targets[2]);
}

#[test]
fn text_ensemble_value_projects_immutably_and_commits_as_one_undo_step() {
    let plugins = PluginManager::default();
    let (service, item_id) = text_service().expect("valid Text Ensemble fixture");
    let (operation_id, _) = service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "transform",
        )
        .unwrap();
    let owner = AuthoringPropertyOwner::TextEnsemble {
        item_id,
        operation_id,
    };
    let update = AuthoringPropertyValueUpdate {
        key: "tx".to_string(),
        value: PropertyValue::from(43.0),
        target: AuthoringPropertyValueTarget::Constant,
    };
    let before = service.snapshot().unwrap();
    let before_revision = service.revision().unwrap();
    let projected = TimelineEditorService::project_authored_property_values(
        before.as_ref(),
        owner,
        vec![update.clone()],
    )
    .unwrap();

    assert_eq!(
        text_operations(&before, item_id).expect("Text operations")[0]
            .properties
            .get("tx")
            .unwrap()
            .value(),
        Some(&PropertyValue::from(0.0)),
        "a live preview must not mutate the authoritative Project"
    );
    assert_eq!(
        text_operations(&projected, item_id).expect("Text operations")[0]
            .properties
            .get("tx")
            .unwrap()
            .value(),
        Some(&PropertyValue::from(43.0))
    );

    service
        .apply_authored_property_values(owner, vec![update])
        .unwrap();
    assert_eq!(
        service.revision().unwrap().get(),
        before_revision.get() + 1,
        "pointer release must produce one transaction"
    );
    let committed = service.snapshot().unwrap();
    assert_eq!(
        text_operations(&committed, item_id).expect("Text operations")[0]
            .properties
            .get("tx")
            .unwrap()
            .value(),
        text_operations(&projected, item_id).expect("Text operations")[0]
            .properties
            .get("tx")
            .unwrap()
            .value(),
        "preview and release must use the same property projection"
    );

    service
        .undo()
        .unwrap()
        .expect("one Text Ensemble value edit");
    let restored = service.snapshot().unwrap();
    assert_eq!(
        text_operations(&restored, item_id).expect("Text operations")[0]
            .properties
            .get("tx")
            .unwrap()
            .value(),
        Some(&PropertyValue::from(0.0))
    );
}

#[test]
fn authoring_fixture_preview_pixels_reflect_opacity_effector() {
    let plugins = Arc::new(PluginManager::default());
    let media_directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test_data/e2e_media");
    let fixture = build_authoring_e2e_fixture(&media_directory, plugins.as_ref()).unwrap();
    let baseline_project = fixture.service.snapshot().unwrap();
    let baseline_plan = RenderPlanCompiler::compile(&baseline_project).unwrap();
    let baseline_frame = evaluate_render_plan_frame(
        &baseline_project,
        &baseline_plan,
        plugins.as_ref(),
        45,
        1.0,
        None,
    )
    .unwrap();

    let (opacity_id, _) = fixture
        .service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            fixture.info.text_item_id,
            TextEnsembleOperationKind::Effector,
            "opacity",
        )
        .unwrap();
    fixture
        .service
        .set_text_ensemble_property(
            plugins.as_ref(),
            fixture.info.text_item_id,
            opacity_id,
            "opacity",
            MediaTime::new(1, 2).unwrap(),
            PropertyValue::from(58.0),
        )
        .unwrap();
    let project = fixture.service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame =
        evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 45, 1.0, None).unwrap();
    assert!(matches!(
        find_text_ensemble(&frame.items)
            .expect("QA Text must reach frame IR")
            .effector_configs
            .as_slice(),
        [EffectorConfig::Opacity {
            target_opacity,
            ..
        }] if *target_opacity == 58.0
    ));

    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(
        640,
        360,
        Color::black(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )
    .unwrap();
    let mut renderer = RenderService::new(renderer, Arc::clone(&plugins), cache);
    let RenderOutput::Image(baseline) = renderer
        .render_authoring_frame(
            &baseline_project,
            &baseline_frame,
            RenderDestination::Preview,
        )
        .unwrap()
    else {
        panic!("authoring render must terminate to an Image");
    };
    let RenderOutput::Image(edited) = renderer
        .render_authoring_frame(&project, &frame, RenderDestination::Preview)
        .unwrap()
    else {
        panic!("authoring render must terminate to an Image");
    };
    assert!(
        edited.data != baseline.data,
        "authoring_e2e Preview pixels must reflect the Opacity Effector"
    );
}

fn find_text_ensemble(items: &[FrameItem]) -> Option<&library::core::ensemble::EnsembleData> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => match &object.content {
            FrameContent::Text { ensemble, .. } => ensemble.as_ref(),
            _ => None,
        },
        FrameItem::Group(group) => find_text_ensemble(&group.items),
    })
}
