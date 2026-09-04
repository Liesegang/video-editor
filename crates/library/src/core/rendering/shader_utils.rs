use skia_safe::RuntimeEffect;

pub const STANDARD_UNIFORMS: &str = r#"
uniform float3 iResolution;
uniform float iTime;
uniform float iTimeDelta;
uniform float iFrame;
uniform float4 iMouse;
uniform float4 iDate;
"#;

pub fn preprocess_shader(code: &str) -> String {
    let source = code
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.starts_with("#version")
                && !line.starts_with("#extension")
                && !line.starts_with("#line")
                && !line.starts_with("#pragma")
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("{STANDARD_UNIFORMS}\n{source}")
}

pub struct ShaderContext {
    pub resolution: (f32, f32),
    pub time: f32,
    pub time_delta: f32,
    pub frame: f32,
    pub mouse: (f32, f32, f32, f32),
    pub date: (f32, f32, f32, f32),
}

pub fn bind_standard_uniforms(effect: &RuntimeEffect, data: &mut [u8], ctx: &ShaderContext) {
    let mut write_f32 = |offset: usize, val: f32| {
        if offset + 4 <= data.len() {
            let bytes = val.to_le_bytes();
            data[offset..offset + 4].copy_from_slice(&bytes);
        }
    };

    for uniform in effect.uniforms() {
        let offset = uniform.offset();
        let name = uniform.name();

        match name {
            "iResolution" => {
                write_f32(offset, ctx.resolution.0);
                write_f32(offset + 4, ctx.resolution.1);
                write_f32(offset + 8, 1.0);
            }
            "iTime" => {
                write_f32(offset, ctx.time);
            }
            "iTimeDelta" => {
                write_f32(offset, ctx.time_delta);
            }
            "iFrame" => {
                write_f32(offset, ctx.frame);
            }
            "iMouse" => {
                write_f32(offset, ctx.mouse.0);
                write_f32(offset + 4, ctx.mouse.1);
                write_f32(offset + 8, ctx.mouse.2);
                write_f32(offset + 12, ctx.mouse.3);
            }
            "iDate" => {
                write_f32(offset, ctx.date.0);
                write_f32(offset + 4, ctx.date.1);
                write_f32(offset + 8, ctx.date.2);
                write_f32(offset + 12, ctx.date.3);
            }
            "iChannelTime" => {
                write_f32(offset, ctx.time);
                write_f32(offset + 4, ctx.time);
                write_f32(offset + 8, ctx.time);
                write_f32(offset + 12, ctx.time);
            }
            _ => {}
        }
    }
}
