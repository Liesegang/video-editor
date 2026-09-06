use super::*;
use crate::model::property::{GradientValue, PropertyValue};
use ordered_float::OrderedFloat;

fn heat_program() -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(vec![
            PointAttributeDefinition::new(
                PointAttributeId::new(),
                "heat",
                PointAttributeElementType::Number,
                PropertyValue::Number(OrderedFloat(0.0)),
            )
            .unwrap(),
        ])
        .unwrap(),
        instructions: vec![
            PointInstruction::NormalizedAge,
            PointInstruction::StoreNumber {
                attribute: 0,
                value: 0,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::Constant {
                value: PropertyValue::Number(OrderedFloat(2.0)),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Multiply,
                left: 2,
                right: 3,
            },
            PointInstruction::ColorRamp {
                gradient: 0,
                factor: 4,
            },
        ],
        ramps: vec![GradientValue::default()],
        color_register: 5,
    }
}

#[test]
fn render_stage_heat_program_is_typed_bounded_and_serializable_as_frame_command() {
    let program = heat_program();
    program.validate().unwrap();
    let decoded: PointRenderProgram =
        serde_json::from_str(&serde_json::to_string(&program).unwrap()).unwrap();
    assert_eq!(decoded, program);
    decoded.validate().unwrap();
}

#[test]
fn point_program_rejects_forward_registers_wrong_types_and_unwritten_attributes() {
    let mut program = heat_program();
    program.instructions[1] = PointInstruction::StoreNumber {
        attribute: 0,
        value: 5,
    };
    assert!(program.validate().unwrap_err().contains("earlier"));
    program = heat_program();
    program.instructions[4] = PointInstruction::LoadAttribute { attribute: 1 };
    assert!(program.validate().unwrap_err().contains("earlier Store"));
    program = heat_program();
    program.color_register = 4;
    assert!(program.validate().unwrap_err().contains("requires Color"));
    program = heat_program();
    program.instructions[0] = PointInstruction::LoadAttribute { attribute: 0 };
    assert!(program.validate().unwrap_err().contains("earlier Store"));
}

#[test]
fn point_program_rejects_nonfinite_constants_and_out_of_range_resources() {
    let mut program = heat_program();
    program.instructions[3] = PointInstruction::Constant {
        value: PropertyValue::Number(OrderedFloat(f64::INFINITY)),
    };
    assert!(program.validate().is_err());
    program = heat_program();
    program.instructions[0] = PointInstruction::Random { channel: 3 };
    assert!(program.validate().unwrap_err().contains("channel"));
    program = heat_program();
    program.instructions[5] = PointInstruction::ColorRamp {
        gradient: 1,
        factor: 4,
    };
    assert!(program.validate().unwrap_err().contains("missing Gradient"));
    program = heat_program();
    program
        .instructions
        .resize(POINT_MAX_INSTRUCTIONS + 1, PointInstruction::Age);
    assert!(program.validate().unwrap_err().contains("instructions"));
}
