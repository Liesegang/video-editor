use sha2::{Digest, Sha256};

use crate::gpu::GpuShaderProgram;
use crate::{
    ColorManagementError, CompiledTransformIdentity, GpuColorTransform, GpuShaderLanguage,
    StandardPrimaries, StandardTransfer, TransformPurpose, TransformSpec,
    standard_hdr::{
        HLG_A, HLG_B, HLG_C, HLG_LINEAR_FACTOR, HLG_NONLINEAR_FACTOR, HLG_SCENE_BREAKPOINT,
        HLG_SIGNAL_BREAKPOINT, ST2084_C1, ST2084_C2, ST2084_C3, ST2084_M1, ST2084_M2,
        ST2084_MAX_NITS,
    },
    standard_spaces::{
        BT709_TRANSFER, BT2020_10_BIT, BT2020_12_BIT, BT2020_ENCODE_EXPONENT, BT2020_EXACT,
        BT2020_LINEAR_SLOPE, Bt2020TransferCoefficients, CompiledStandardTransform, SRGB_TRANSFER,
        SdrPowerTransferCoefficients, from_xyz_matrix, to_xyz_matrix,
    },
};

pub(crate) fn extract_standard_gpu_transform(
    transform: CompiledStandardTransform,
    identity: CompiledTransformIdentity,
    language: GpuShaderLanguage,
) -> Result<GpuColorTransform, ColorManagementError> {
    if language != GpuShaderLanguage::Glsl {
        return Err(ColorManagementError::GpuTransformUnavailable {
            backend_id: identity.cache_key().backend_id.clone(),
        });
    }

    let prefix = shader_prefix(&identity);
    let entry_point = format!("{prefix}_transform_rgb");
    let domain_entry_point = format!("{prefix}_domain_valid");
    let source = generate_glsl(&transform, &prefix, &entry_point, &domain_entry_point)?;
    GpuColorTransform::new(
        identity,
        language,
        transform.source().id.as_str().to_string(),
        transform.destination().id.as_str().to_string(),
        GpuShaderProgram {
            source,
            entry_point,
            domain_entry_point,
            luts: Vec::new(),
        },
    )
}

fn shader_prefix(identity: &CompiledTransformIdentity) -> String {
    let key = identity.cache_key();
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, key.backend_id.as_bytes());
    hash_field(&mut hasher, key.config_fingerprint.as_bytes());
    hash_field(&mut hasher, purpose_name(key.purpose).as_bytes());
    match &key.spec {
        TransformSpec::ColorSpace {
            source,
            destination,
        } => {
            hash_field(&mut hasher, b"color-space");
            hash_field(&mut hasher, source.as_bytes());
            hash_field(&mut hasher, destination.as_bytes());
        }
        TransformSpec::DisplayView {
            source,
            display,
            view,
            looks_bypass,
            data_bypass,
        } => {
            hash_field(&mut hasher, b"display-view");
            hash_field(&mut hasher, source.as_bytes());
            hash_field(&mut hasher, display.as_bytes());
            hash_field(&mut hasher, view.as_bytes());
            hash_field(&mut hasher, &[*looks_bypass as u8, *data_bypass as u8]);
        }
    }
    hash_field(&mut hasher, key.context.fingerprint().as_bytes());
    hash_field(&mut hasher, identity.backend_program_cache_id().as_bytes());
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut suffix = String::with_capacity(24);
    for byte in &digest[..12] {
        suffix.push(char::from(HEX[usize::from(byte >> 4)]));
        suffix.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    format!("ruvie_color_{suffix}")
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(value.len().to_le_bytes());
    hasher.update(value);
}

const fn purpose_name(purpose: TransformPurpose) -> &'static str {
    match purpose {
        TransformPurpose::Explicit => "explicit",
        TransformPurpose::SourceToWorking => "source-to-working",
        TransformPurpose::WorkingToDisplay => "working-to-display",
        TransformPurpose::WorkingToOutput => "working-to-output",
    }
}

fn generate_glsl(
    transform: &CompiledStandardTransform,
    prefix: &str,
    entry_point: &str,
    domain_entry_point: &str,
) -> Result<String, ColorManagementError> {
    let source = transform.source();
    let destination = transform.destination();
    if source.id == destination.id {
        return Ok(format!(
            "bool {domain_entry_point}(vec3 rgb) {{ return true; }}\nvec3 {entry_point}(vec3 rgb) {{ return rgb; }}\n"
        ));
    }

    let decode = format!("{prefix}_decode");
    let convert = format!("{prefix}_convert");
    let encode = format!("{prefix}_encode");
    let mut glsl = String::new();
    write_transfer_function(&mut glsl, &decode, source.transfer, true, transform)?;
    write_primary_conversion(&mut glsl, &convert, source.primaries, destination.primaries);
    write_transfer_function(&mut glsl, &encode, destination.transfer, false, transform)?;

    glsl.push_str(&format!("bool {domain_entry_point}(vec3 rgb) {{\n"));
    if source.transfer == StandardTransfer::St2084Pq {
        glsl.push_str(
            "  if (any(lessThan(rgb, vec3(0.0))) || any(greaterThan(rgb, vec3(1.0)))) return false;\n",
        );
    }
    if destination.transfer == StandardTransfer::St2084Pq {
        glsl.push_str(&format!("  vec3 linear_rgb = {convert}({decode}(rgb));\n"));
        let reference_white = pq_reference_white(transform)?;
        glsl.push_str(&format!(
            "  vec3 nits = linear_rgb * {};\n",
            glsl_number(reference_white)
        ));
        glsl.push_str(&format!(
            "  if (any(lessThan(nits, vec3(0.0))) || any(greaterThan(nits, vec3({})))) return false;\n",
            glsl_number(ST2084_MAX_NITS)
        ));
    }
    glsl.push_str("  return true;\n}\n");
    glsl.push_str(&format!(
        "vec3 {entry_point}(vec3 rgb) {{ return {encode}({convert}({decode}(rgb))); }}\n"
    ));
    Ok(glsl)
}

fn write_transfer_function(
    glsl: &mut String,
    name: &str,
    transfer: StandardTransfer,
    decode: bool,
    transform: &CompiledStandardTransform,
) -> Result<(), ColorManagementError> {
    let scalar = format!("{name}_component");
    glsl.push_str(&format!("float {scalar}(float value) {{\n"));
    glsl.push_str("  float magnitude = abs(value);\n");
    let expression = transfer_expression(transfer, decode, transform, "magnitude")?;
    glsl.push_str(&format!("  float transformed = {expression};\n"));
    glsl.push_str("  return value < 0.0 ? -transformed : transformed;\n}\n");
    glsl.push_str(&format!(
        "vec3 {name}(vec3 value) {{ return vec3({scalar}(value.r), {scalar}(value.g), {scalar}(value.b)); }}\n"
    ));
    Ok(())
}

fn transfer_expression(
    transfer: StandardTransfer,
    decode: bool,
    transform: &CompiledStandardTransform,
    value: &str,
) -> Result<String, ColorManagementError> {
    let expression = match (transfer, decode) {
        (StandardTransfer::Linear, _) => value.to_string(),
        (StandardTransfer::Srgb, direction) => {
            piecewise_expression(value, SRGB_TRANSFER, direction)
        }
        (StandardTransfer::Bt709, direction) => {
            piecewise_expression(value, BT709_TRANSFER, direction)
        }
        (StandardTransfer::Bt2020Exact, direction) => {
            bt2020_expression(value, BT2020_EXACT, direction)
        }
        (StandardTransfer::Bt2020TenBit, direction) => {
            bt2020_expression(value, BT2020_10_BIT, direction)
        }
        (StandardTransfer::Bt2020TwelveBit, direction) => {
            bt2020_expression(value, BT2020_12_BIT, direction)
        }
        (StandardTransfer::St2084Pq, true) => {
            let reference_white = pq_reference_white(transform)?;
            format!(
                "{} * pow(max(pow({value}, 1.0 / {}) - {}, 0.0) / ({} - {} * pow({value}, 1.0 / {})), 1.0 / {}) / {}",
                glsl_number(ST2084_MAX_NITS),
                glsl_number(ST2084_M2),
                glsl_number(ST2084_C1),
                glsl_number(ST2084_C2),
                glsl_number(ST2084_C3),
                glsl_number(ST2084_M2),
                glsl_number(ST2084_M1),
                glsl_number(reference_white),
            )
        }
        (StandardTransfer::St2084Pq, false) => {
            let reference_white = pq_reference_white(transform)?;
            let normalized = format!(
                "pow(({value} * {}) / {}, {})",
                glsl_number(reference_white),
                glsl_number(ST2084_MAX_NITS),
                glsl_number(ST2084_M1)
            );
            format!(
                "pow(({} + {} * {normalized}) / (1.0 + {} * {normalized}), {})",
                glsl_number(ST2084_C1),
                glsl_number(ST2084_C2),
                glsl_number(ST2084_C3),
                glsl_number(ST2084_M2)
            )
        }
        (StandardTransfer::Bt2100Hlg, true) => format!(
            "{value} <= {} ? {value} * {value} / {} : (exp(({value} - {}) / {}) + {}) / {}",
            glsl_number(HLG_SIGNAL_BREAKPOINT),
            glsl_number(HLG_LINEAR_FACTOR),
            glsl_number(HLG_C),
            glsl_number(HLG_A),
            glsl_number(HLG_B),
            glsl_number(HLG_NONLINEAR_FACTOR)
        ),
        (StandardTransfer::Bt2100Hlg, false) => format!(
            "{value} <= {} ? sqrt({} * {value}) : {} * log({} * {value} - {}) + {}",
            glsl_number(HLG_SCENE_BREAKPOINT),
            glsl_number(HLG_LINEAR_FACTOR),
            glsl_number(HLG_A),
            glsl_number(HLG_NONLINEAR_FACTOR),
            glsl_number(HLG_B),
            glsl_number(HLG_C)
        ),
    };
    Ok(expression)
}

fn pq_reference_white(transform: &CompiledStandardTransform) -> Result<f64, ColorManagementError> {
    transform
        .hdr_context()
        .pq_reference_white_nits()
        .ok_or_else(|| ColorManagementError::ProcessorContractMismatch {
            operation: "standard GPU color transform generation",
            detail: "compiled PQ context is incomplete".to_string(),
        })
}

fn piecewise_expression(
    value: &str,
    coefficients: SdrPowerTransferCoefficients,
    decode: bool,
) -> String {
    let slope = glsl_number(coefficients.linear_slope);
    let scale = glsl_number(coefficients.nonlinear_scale);
    let offset = glsl_number(coefficients.nonlinear_offset);
    if decode {
        format!(
            "{value} <= {} ? {value} / {slope} : pow(({value} + {offset}) / {scale}, {})",
            glsl_number(coefficients.encoded_threshold),
            glsl_number(coefficients.decode_exponent)
        )
    } else {
        format!(
            "{value} <= {} ? {value} * {slope} : {scale} * pow({value}, {}) - {offset}",
            glsl_number(coefficients.linear_threshold),
            glsl_number(coefficients.encode_exponent)
        )
    }
}

fn bt2020_expression(
    value: &str,
    coefficients: Bt2020TransferCoefficients,
    decode: bool,
) -> String {
    let alpha = glsl_number(coefficients.alpha);
    let beta = glsl_number(coefficients.beta);
    let slope = glsl_number(BT2020_LINEAR_SLOPE);
    let exponent = glsl_number(BT2020_ENCODE_EXPONENT);
    if decode {
        format!(
            "{value} < {beta} * {slope} ? {value} / {slope} : pow(({value} + {alpha} - 1.0) / {alpha}, 1.0 / {exponent})"
        )
    } else {
        format!(
            "{value} < {beta} ? {value} * {slope} : {alpha} * pow({value}, {exponent}) - ({alpha} - 1.0)"
        )
    }
}

fn write_primary_conversion(
    glsl: &mut String,
    name: &str,
    source: StandardPrimaries,
    destination: StandardPrimaries,
) {
    if source == destination {
        glsl.push_str(&format!("vec3 {name}(vec3 value) {{ return value; }}\n"));
        return;
    }
    let to_xyz = format!("{name}_to_xyz");
    write_matrix_function(glsl, &to_xyz, to_xyz_matrix(source));
    let from_xyz = format!("{name}_from_xyz");
    write_matrix_function(glsl, &from_xyz, from_xyz_matrix(destination));
    glsl.push_str(&format!(
        "vec3 {name}(vec3 value) {{ return {from_xyz}({to_xyz}(value)); }}\n"
    ));
}

fn write_matrix_function(glsl: &mut String, name: &str, matrix: [[f64; 3]; 3]) {
    glsl.push_str(&format!("vec3 {name}(vec3 value) {{\n"));
    glsl.push_str(&format!(
        "  return vec3(dot(vec3({}, {}, {}), value), dot(vec3({}, {}, {}), value), dot(vec3({}, {}, {}), value));\n",
        glsl_number(matrix[0][0]), glsl_number(matrix[0][1]), glsl_number(matrix[0][2]),
        glsl_number(matrix[1][0]), glsl_number(matrix[1][1]), glsl_number(matrix[1][2]),
        glsl_number(matrix[2][0]), glsl_number(matrix[2][1]), glsl_number(matrix[2][2]),
    ));
    glsl.push_str("}\n");
}

fn glsl_number(value: f64) -> String {
    format!("{value:.17e}")
}

#[cfg(test)]
#[path = "gpu_standard_tests.rs"]
mod tests;
