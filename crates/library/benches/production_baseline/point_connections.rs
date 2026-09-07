//! Deterministic warm-GPU fixtures for the production Point connection path.

use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;

use library::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use library::editor::{
    ModuleItemPlacement, PlexusNodeClipDefinition, PlexusNodeClipFactory,
    PlexusPublishedParameters, RenderDestination, TimelineEditorService,
};
use library::model::authoring::{
    AuthoringProject, MediaTime, ModuleConnectionId, ModuleDefinitionId, ModuleInstanceId,
    ModuleOutputId, PublishedParameterId, RationalRate, SourceRef, TimelineInterval,
    TimelineItemId,
};
use library::model::node::{Node, NodeContent};
use library::model::property::{PropertyValue, Vec3};
use library::plugin::PluginManager;
use library::rendering::skia_utils::GpuDriverInfo;
use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::BenchResult;
use crate::fixtures::{canonical_project_json, stabilize_root_ids, stable_uuid};
use crate::gpu_preview::{validate_driver, warmed_gpu_service};
use crate::report::{MetricDefinition, MetricResult, RunConfiguration, measure, unavailable};

const FIXTURE_NAMESPACE_BASE: u16 = 100;
const GRID_SPACING: f64 = 24.0;
const MAX_DISTANCE: f64 = 25.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PointLineSpec {
    pub name: &'static str,
    pub fixture: &'static str,
    pub counts: [u32; 3],
    pub max_neighbors: u32,
}

const POINT_LINE_SPECS: [PointLineSpec; 5] = [
    PointLineSpec {
        name: "gpu_preview_point_lines_1000_k6",
        fixture: "point-lines-grid-1000-k6",
        counts: [10, 10, 10],
        max_neighbors: 6,
    },
    PointLineSpec {
        name: "gpu_preview_point_lines_10000_k6",
        fixture: "point-lines-grid-10000-k6",
        counts: [25, 20, 20],
        max_neighbors: 6,
    },
    PointLineSpec {
        name: "gpu_preview_point_lines_100000_k6",
        fixture: "point-lines-grid-100000-k6",
        counts: [50, 50, 40],
        max_neighbors: 6,
    },
    PointLineSpec {
        name: "gpu_preview_point_lines_100000_k1",
        fixture: "point-lines-grid-100000-k1",
        counts: [50, 50, 40],
        max_neighbors: 1,
    },
    PointLineSpec {
        name: "gpu_preview_point_lines_100000_k32",
        fixture: "point-lines-grid-100000-k32",
        counts: [50, 50, 40],
        max_neighbors: 32,
    },
];

pub(super) struct PointLineFixture {
    pub spec: PointLineSpec,
    pub project: AuthoringProject,
}

pub(super) fn build_fixtures() -> BenchResult<Vec<PointLineFixture>> {
    POINT_LINE_SPECS
        .into_iter()
        .enumerate()
        .map(|(index, spec)| {
            Ok(PointLineFixture {
                spec,
                project: build_fixture(spec, FIXTURE_NAMESPACE_BASE + index as u16)?,
            })
        })
        .collect()
}

pub(super) fn run(
    fixtures: &[PointLineFixture],
    plugins: &Arc<PluginManager>,
    configuration: RunConfiguration,
    driver: &mut Option<GpuDriverInfo>,
) -> BenchResult<Vec<MetricResult>> {
    fixtures
        .iter()
        .map(|fixture| {
            let plan = RenderPlanCompiler::compile(&fixture.project)?;
            let frame = evaluate_render_plan_frame(
                &fixture.project,
                &plan,
                plugins.as_ref(),
                0,
                1.0,
                None,
            )?;
            let (mut service, actual_driver) =
                warmed_gpu_service(&fixture.project, &frame, plugins)?;
            validate_driver(driver, actual_driver)?;
            measure(metric_definition(fixture.spec), configuration, || {
                black_box(service.render_authoring_frame(
                    &fixture.project,
                    &frame,
                    RenderDestination::Preview,
                )?);
                Ok(())
            })
        })
        .collect()
}

pub(super) fn unavailable_metrics(reason: &str) -> Vec<MetricResult> {
    POINT_LINE_SPECS
        .into_iter()
        .map(|spec| {
            unavailable(
                spec.name,
                "point_connections",
                "Warm OpenGL Preview of a Grid connected by exact nearest-K Point Lines",
                "RenderPlan Point Grid -> Connect Points -> Line Renderer -> RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
                reason,
            )
        })
        .collect()
}

pub(super) fn contract_self_check() -> BenchResult<()> {
    let fixtures = build_fixtures()?;
    let repeated = build_fixtures()?;
    for (fixture, repeated) in fixtures.into_iter().zip(repeated) {
        let expected = fixture.spec.counts.into_iter().product::<u32>();
        let json = canonical_project_json(&fixture.project)?;
        if !json.contains(fixture.spec.fixture)
            || expected == 0
            || json != canonical_project_json(&repeated.project)?
        {
            return Err(format!(
                "{} Point Line fixture identity is incomplete",
                fixture.spec.fixture
            )
            .into());
        }
        fixture.project.validate()?;
    }
    Ok(())
}

fn metric_definition(spec: PointLineSpec) -> MetricDefinition<'static> {
    MetricDefinition {
        name: spec.name,
        category: "point_connections",
        description: "Warm OpenGL Preview of a Grid connected by exact nearest-K Point Lines",
        production_path: "RenderPlan Point Grid -> Connect Points -> Line Renderer -> RenderService<SkiaRenderer>::render_authoring_frame(Preview)",
        fixture: spec.fixture,
        operations_per_sample: 1,
    }
}

fn build_fixture(spec: PointLineSpec, namespace: u16) -> BenchResult<AuthoringProject> {
    let duration = MediaTime::from_whole_seconds(10);
    let mut project = AuthoringProject::new(
        spec.fixture,
        1920,
        1080,
        RationalRate::new(30, 1)?,
        duration,
    )?;
    let track_id = stabilize_root_ids(&mut project, namespace)?;
    let plexus = stabilize_plexus(PlexusNodeClipFactory::create(spec.fixture)?, namespace)?;
    let definition_id = plexus.definition.id;
    let service = TimelineEditorService::new(project)?;
    service.add_module_definition(plexus.definition)?;
    let (item_id, instance_id, _) = service.place_module_item(
        definition_id,
        ModuleItemPlacement {
            track_id,
            name: spec.fixture.to_string(),
            output_id: plexus.output_id,
            interval: TimelineInterval::new(MediaTime::zero(), duration)?,
            layer: 0,
            parameter_overrides: point_line_overrides(plexus.parameters, spec),
            input_bindings: HashMap::new(),
        },
    )?;
    let mut project = service.snapshot()?.as_ref().clone();
    stabilize_placement(&mut project, item_id, instance_id, namespace)?;
    project.validate()?;
    Ok(project)
}

fn point_line_overrides(
    parameters: PlexusPublishedParameters,
    spec: PointLineSpec,
) -> HashMap<PublishedParameterId, PropertyValue> {
    HashMap::from([
        (
            parameters.count_x,
            PropertyValue::Integer(i64::from(spec.counts[0])),
        ),
        (
            parameters.count_y,
            PropertyValue::Integer(i64::from(spec.counts[1])),
        ),
        (
            parameters.count_z,
            PropertyValue::Integer(i64::from(spec.counts[2])),
        ),
        (
            parameters.spacing,
            PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(GRID_SPACING),
                y: OrderedFloat(GRID_SPACING),
                z: OrderedFloat(GRID_SPACING),
            }),
        ),
        (parameters.min_distance, PropertyValue::from(0.0)),
        (parameters.max_distance, PropertyValue::from(MAX_DISTANCE)),
        (
            parameters.max_neighbors,
            PropertyValue::Integer(i64::from(spec.max_neighbors)),
        ),
    ])
}

fn stabilize_plexus(
    mut plexus: PlexusNodeClipDefinition,
    namespace: u16,
) -> BenchResult<PlexusNodeClipDefinition> {
    let definition_id = ModuleDefinitionId::from_uuid(stable_uuid(namespace, 100));
    let output_id = ModuleOutputId::from_uuid(stable_uuid(namespace, 101));
    let mut node_ids = HashMap::new();
    for node in plexus.definition.graph.nodes.values() {
        let offset = match node.content() {
            NodeContent::NativeOperation(operation) => match operation.catalog_id.as_str() {
                "native.point.grid" => 110,
                "native.point.connect-points" => 111,
                "native.point.line-renderer" => 112,
                other => return Err(format!("unexpected Plexus Node '{other}'").into()),
            },
            NodeContent::ModuleOutput(_) => 113,
            _ => return Err("Plexus fixture contains an unexpected Node kind".into()),
        };
        node_ids.insert(node.id, stable_uuid(namespace, offset));
    }

    let old_nodes = std::mem::take(&mut plexus.definition.graph.nodes);
    for (old_id, mut node) in old_nodes {
        let new_id = node_ids[&old_id];
        if matches!(node.content(), NodeContent::ModuleOutput(_)) {
            node = stable_output_node(node, new_id, output_id)?;
        } else {
            node.id = new_id;
        }
        plexus.definition.graph.nodes.insert(new_id, node);
    }
    for (index, connection) in plexus.definition.graph.connections.iter_mut().enumerate() {
        connection.id = ModuleConnectionId::from_uuid(stable_uuid(namespace, 120 + index as u64));
        connection.from.node_id = node_ids[&connection.from.node_id];
        connection.to.node_id = node_ids[&connection.to.node_id];
    }

    let mut parameter_ids = HashMap::new();
    for (index, parameter) in plexus
        .definition
        .interface
        .parameters
        .iter_mut()
        .enumerate()
    {
        let old_id = parameter.id;
        parameter.id = PublishedParameterId::from_uuid(stable_uuid(namespace, 140 + index as u64));
        parameter.target.node_id = node_ids[&parameter.target.node_id];
        parameter_ids.insert(old_id, parameter.id);
    }
    plexus.definition.id = definition_id;
    plexus.output_id = output_id;
    plexus.parameters = remap_parameters(plexus.parameters, &parameter_ids)?;
    plexus.definition.validate()?;
    Ok(plexus)
}

fn stable_output_node(node: Node, node_id: Uuid, output_id: ModuleOutputId) -> BenchResult<Node> {
    // Node content is intentionally immutable through authoring APIs. This
    // fixture round-trip changes only the two persisted identities so repeated
    // benchmark construction has byte-identical canonical serialization.
    let mut value = serde_json::to_value(node)?;
    value["id"] = serde_json::to_value(node_id)?;
    value["content"]["data"]["id"] = serde_json::to_value(output_id)?;
    Ok(serde_json::from_value(value)?)
}

fn remap_parameters(
    parameters: PlexusPublishedParameters,
    ids: &HashMap<PublishedParameterId, PublishedParameterId>,
) -> BenchResult<PlexusPublishedParameters> {
    let get = |id| -> BenchResult<PublishedParameterId> {
        ids.get(&id)
            .copied()
            .ok_or_else(|| format!("Plexus fixture lost Published parameter {id}").into())
    };
    Ok(PlexusPublishedParameters {
        count_x: get(parameters.count_x)?,
        count_y: get(parameters.count_y)?,
        count_z: get(parameters.count_z)?,
        spacing: get(parameters.spacing)?,
        center: get(parameters.center)?,
        min_distance: get(parameters.min_distance)?,
        max_distance: get(parameters.max_distance)?,
        max_neighbors: get(parameters.max_neighbors)?,
        color: get(parameters.color)?,
        width: get(parameters.width)?,
        fade: get(parameters.fade)?,
    })
}

fn stabilize_placement(
    project: &mut AuthoringProject,
    old_item_id: TimelineItemId,
    old_instance_id: ModuleInstanceId,
    namespace: u16,
) -> BenchResult<()> {
    let instance_id = ModuleInstanceId::from_uuid(stable_uuid(namespace, 200));
    let item_id = TimelineItemId::from_uuid(stable_uuid(namespace, 201));
    let mut instance = project
        .module_instances
        .remove(&old_instance_id)
        .ok_or("Plexus fixture placement lost its Module instance")?;
    instance.id = instance_id;
    project.module_instances.insert(instance_id, instance);
    let mut item = project
        .items
        .remove(&old_item_id)
        .ok_or("Plexus fixture placement lost its Timeline item")?;
    item.id = item_id;
    let SourceRef::Module(invocation) = &mut item.source else {
        return Err("Plexus fixture placement is not a Module invocation".into());
    };
    invocation.instance_id = instance_id;
    project.items.insert(item_id, item);
    Ok(())
}
