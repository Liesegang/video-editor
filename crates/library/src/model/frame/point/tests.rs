use super::*;
use crate::model::point::{PointAttributeSchema, PointInstruction};
use crate::model::property::PropertyValue;

fn grid() -> PointGridParameters {
    PointGridParameters {
        counts: [8, 8, 1],
        spacing: Vec3 {
            x: 24.0.into(),
            y: 24.0.into(),
            z: 24.0.into(),
        },
        center: Vec3 {
            x: 0.0.into(),
            y: 0.0.into(),
            z: 0.0.into(),
        },
        size: 8.0.into(),
        seed: 1,
    }
}

#[test]
fn grid_validation_rejects_invalid_allocation_and_nonfinite_geometry() {
    grid().validate().unwrap();
    for counts in [[0, 8, 1], [1025, 1, 1], [100, 100, 11], [u32::MAX; 3]] {
        let mut value = grid();
        value.counts = counts;
        assert!(value.validate().is_err());
    }
    let mut value = grid();
    value.counts = [100, 100, 10];
    value.validate().unwrap();
    assert_eq!(value.capacity(), POINT_MAX_CAPACITY);
    for bad in [f64::NAN, f64::INFINITY, 1e40] {
        value.spacing.x = bad.into();
        assert!(value.validate().is_err());
    }
    value = grid();
    value.center.x = (POINT_GRID_POSITION_LIMIT - 1.0).into();
    assert!(value.validate().unwrap_err().contains("positions"));
    value = grid();
    value.spacing.x = (-24.0).into();
    value.validate().unwrap();
    for bad in [f32::NAN, f32::INFINITY, 0.0, -1.0, 513.0] {
        value.size = bad.into();
        assert!(value.validate().is_err());
    }
}

#[test]
fn grid_point_identity_is_exact_unique_and_stable_across_count_changes() {
    let mut value = grid();
    value.counts = [20, 20, 20];
    let mut serials = std::collections::HashSet::new();
    for z in 0..20 {
        for y in 0..20 {
            for x in 0..20 {
                assert!(serials.insert(value.point_serial([x, y, z]).unwrap()));
            }
        }
    }
    let before = value.point_serial([3, 5, 7]).unwrap();
    value.counts = [15, 16, 17];
    assert_eq!(value.point_serial([3, 5, 7]), Some(before));
    assert_eq!(before, 3 | (5 << 10) | (7 << 20));
    assert_eq!(value.point_serial([15, 5, 7]), None);
    value.counts = [1024, 1, 1];
    value.validate().unwrap();
    assert_eq!(value.point_serial([1023, 0, 0]), Some(1023));
}

#[test]
fn grid_source_round_trips_without_simulation_history() {
    let source = PointSceneSource::Grid(grid());
    let json = serde_json::to_string(&source).unwrap();
    assert!(!json.contains("target_step"));
    assert!(!json.contains("lifetime"));
    let restored: PointSceneSource = serde_json::from_str(&json).unwrap();
    assert_eq!(source, restored);
    assert_eq!(source.capacity(), 64);
    assert_eq!(source.seed(), 1);
    assert!(!source.supports_age());
}

#[test]
fn point_frame_rejects_particle_builtins_on_grid_before_gpu_dispatch() {
    let mut frame = PointSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: InstancePath::root(crate::model::authoring::TimelineId::new()),
            module_instance_id: ModuleInstanceId::new(),
            state_slot_id: Uuid::new_v4(),
            output_id: ModuleOutputId::new(),
        },
        source_node_id: Uuid::new_v4(),
        executable_hash: [0; 32],
        logical_width: 640,
        logical_height: 480,
        source: PointSceneSource::Grid(grid()),
        color: Color::white(),
        point_program: None,
    };
    frame.validate().unwrap();
    for instruction in [PointInstruction::Age, PointInstruction::NormalizedAge] {
        frame.point_program = Some(PointRenderProgram {
            schema: PointAttributeSchema::new(Vec::new()).unwrap(),
            instructions: vec![
                instruction,
                PointInstruction::Constant {
                    value: PropertyValue::ColorValue(
                        crate::model::property::ColorValue::from_straight_srgba8(&Color::white()),
                    ),
                },
            ],
            ramps: Vec::new(),
            color_register: 1,
            position_register: None,
        });
        assert!(frame.validate().unwrap_err().contains("require a Particle"));
    }
}
