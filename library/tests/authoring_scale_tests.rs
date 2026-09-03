use std::collections::HashMap;
use std::time::{Duration, Instant};

use library::core::render_plan::RenderPlanCompiler;
use library::model::authoring::{
    AuthoringProject, AuthoringSession, ModuleDefinition, ModuleDefinitionId, ModuleGraph,
    ModuleInstance, ModuleInstanceId, ModuleRole, SourceRef, TimelineInterval, TimelineItem,
    TimelineItemId,
};
use library::model::project::property::PropertyMap;

const TEN_THOUSAND_ITEM_COMPILE_BUDGET: Duration = Duration::from_secs(2);

#[test]
fn timeline_item_scale_does_not_create_user_or_runtime_nodes() {
    for item_count in [100_usize, 1_000, 10_000] {
        let mut session = AuthoringSession::new(
            AuthoringProject::new("Scale", 1920, 1080, 30.0, 60.0).expect("project"),
        )
        .expect("session");
        let project = session.project();
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        for index in 0..item_count {
            session
                .add_item(
                    track_id,
                    format!("Caption {index}"),
                    SourceRef::Text {
                        text: format!("Line {index}"),
                    },
                    TimelineInterval::new(index as f64 * 0.01, 1.0).expect("interval"),
                    index as i64,
                )
                .expect("Timeline item");
        }
        let project = session.into_project();
        let started = Instant::now();
        let plan = RenderPlanCompiler::compile(&project).expect("RenderPlan");
        let elapsed = started.elapsed();
        assert_eq!(plan.timelines[&timeline_id].schedule.len(), item_count);
        assert!(plan.module_definitions.is_empty());
        assert!(plan.module_invocations.is_empty());
        eprintln!(
            "RenderPlan baseline: {item_count} TimelineItems compiled in {:.3} ms",
            elapsed.as_secs_f64() * 1_000.0
        );
        if item_count == 10_000 {
            assert!(
                elapsed <= TEN_THOUSAND_ITEM_COMPILE_BUDGET,
                "10,000 TimelineItems exceeded the {:?} RenderPlan compile budget: {:?}",
                TEN_THOUSAND_ITEM_COMPILE_BUDGET,
                elapsed
            );
        }
    }
}

#[test]
fn ten_thousand_instances_share_one_compiled_module_definition() {
    let mut project =
        AuthoringProject::new("Shared modules", 1920, 1080, 30.0, 120.0).expect("project");
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let definition_id = ModuleDefinitionId::new();
    project.module_definitions.insert(
        definition_id,
        ModuleDefinition {
            id: definition_id,
            name: "Shared lower third".to_string(),
            role: ModuleRole::Generator,
            graph: ModuleGraph::default(),
            output_node_id: None,
            published_parameters: Vec::new(),
            published_signals: Vec::new(),
            published_actions: Vec::new(),
            version: 1,
        },
    );
    for index in 0..10_000 {
        let instance_id = ModuleInstanceId::new();
        project.module_instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                definition_id,
                parameter_overrides: HashMap::new(),
            },
        );
        let item_id = TimelineItemId::new();
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Lower third {index}"),
                source: SourceRef::Module {
                    module_instance_id: instance_id,
                },
                interval: TimelineInterval::new(index as f64 * 0.01, 1.0).expect("interval"),
                layer: index,
                parent: None,
                mask_ids: Vec::new(),
                matte: None,
                constraints: Vec::new(),
                transition_in: None,
                transition_out: None,
                generated_item_id: None,
                authored_properties: PropertyMap::new(),
            },
        );
    }

    let started = Instant::now();
    let plan = RenderPlanCompiler::compile(&project).expect("RenderPlan");
    let elapsed = started.elapsed();
    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 10_000);
    assert_eq!(plan.timelines[&timeline_id].schedule.len(), 10_000);
    assert!(
        elapsed <= TEN_THOUSAND_ITEM_COMPILE_BUDGET,
        "10,000 shared Module instances exceeded the {:?} RenderPlan compile budget: {:?}",
        TEN_THOUSAND_ITEM_COMPILE_BUDGET,
        elapsed
    );
    eprintln!(
        "RenderPlan baseline: 10,000 invocations shared 1 ModuleDefinition in {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    );
}
