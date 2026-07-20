use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ruvie_plugin_api::{
    BackplateShapeV1, ColorV1, ComponentDescriptorV1, DecoratorEvaluateRequestV1,
    DecoratorOutputV1, DecoratorTargetV1, InsetsV1, InvokeRequestV1, PluginDescriptorV1,
    PropertyDefinitionV1, PropertyEvaluateRequestV1, PropertyEvaluateResponseV1, PropertyUiV1,
    PropertyValueV1, RuvieAssetMetadataV1, RuvieBuffer, RuvieBytesView, RuvieCallResult,
    RuvieEffectCpuRgba8ApiV1, RuvieExtensionResultV1, RuvieLoaderCpuRgba8ApiV1,
    RuvieLoaderRequestV1, RuvieOwnedRgba8FrameV1, RuviePluginApiV1, RuviePropertyMapViewV1,
    RuvieRgba8FrameViewV1, StrokeCapV1, StrokeJoinV1, StyleEvaluateRequestV1, StyleOutputV1,
    ALPHA_MODE_STRAIGHT_V1, ASSET_KIND_IMAGE_V1, ASSET_KIND_VIDEO_V1, ASSET_METADATA_DIMENSIONS_V1,
    ASSET_METADATA_DURATION_V1, ASSET_METADATA_FPS_V1, ASSET_METADATA_FRAME_COUNT_V1,
    ASSET_METADATA_STREAM_INDEX_V1, ASSET_METADATA_TIME_BASE_V1, COLOR_PROFILE_SRGB_V1,
    DECORATOR_CATEGORY, DECORATOR_EVALUATE_V1, EFFECT_CATEGORY, EFFECT_CPU_RGBA8_EXTENSION_V1,
    EFFECT_PROCESS_CPU_RGBA8_V1, LOADER_CATEGORY, LOADER_CPU_RGBA8_EXTENSION_V1,
    LOADER_LOAD_CPU_RGBA8_V1, LOADER_OPEN_V1, LOAD_REQUEST_IMAGE_V1, LOAD_REQUEST_VIDEO_FRAME_V1,
    MAX_CPU_RGBA8_DIMENSION_V1, MAX_CPU_RGBA8_FRAME_BYTES_V1, MAX_STYLE_DASH_INTERVALS_V1,
    PROPERTY_CATEGORY, PROPERTY_EVALUATE_V1, PROPERTY_VALUE_INTEGER_V1, RUVIE_PLUGIN_ABI_V1,
    STATUS_INVALID_REQUEST, STATUS_PANIC, STATUS_PLUGIN_ERROR, STYLE_CATEGORY, STYLE_EVALUATE_V1,
};

const COMPONENT_ID: &str = "random_property";
const FILL_COMPONENT_ID: &str = "runtime_fill_style";
const STROKE_COMPONENT_ID: &str = "runtime_stroke_style";
const BACKPLATE_COMPONENT_ID: &str = "runtime_backplate_decorator";
const EFFECT_COMPONENT_ID: &str = "runtime_solid_tint_effect";
const LOADER_COMPONENT_ID: &str = "runtime_rgba_fixture_loader";
const DESCRIPTOR_CALLS_OPERATION: &str = "random_property.descriptor_calls.v1";
static DESCRIPTOR_CALLS: AtomicUsize = AtomicUsize::new(0);

fn descriptor() -> PluginDescriptorV1 {
    PluginDescriptorV1 {
        name: "Random Property".to_string(),
        vendor: "RuViE".to_string(),
        version: "0.1.0".to_string(),
        components: vec![
            property_descriptor(),
            fill_descriptor(),
            stroke_descriptor(),
            backplate_descriptor(),
            effect_descriptor(),
            loader_descriptor(),
        ],
    }
}

fn effect_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: EFFECT_COMPONENT_ID.to_string(),
        name: "Runtime Solid Tint".to_string(),
        category: EFFECT_CATEGORY.to_string(),
        group: "Effect/Runtime Fixture".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![EFFECT_PROCESS_CPU_RGBA8_V1.to_string()],
        properties: vec![PropertyDefinitionV1 {
            name: "red".to_string(),
            label: "Red".to_string(),
            ui: PropertyUiV1::Integer {
                min: 0,
                max: 255,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            default: serde_json::json!(220),
        }],
        output_default: None,
    }
}

fn loader_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: LOADER_COMPONENT_ID.to_string(),
        name: "Runtime RGBA Fixture Loader".to_string(),
        category: LOADER_CATEGORY.to_string(),
        group: "Loader/Runtime Fixture".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![
            LOADER_OPEN_V1.to_string(),
            LOADER_LOAD_CPU_RGBA8_V1.to_string(),
        ],
        properties: Vec::new(),
        output_default: None,
    }
}

fn property_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: COMPONENT_ID.to_string(),
        name: "Random Property".to_string(),
        category: PROPERTY_CATEGORY.to_string(),
        group: "Property".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![
            PROPERTY_EVALUATE_V1.to_string(),
            DESCRIPTOR_CALLS_OPERATION.to_string(),
        ],
        properties: vec![
            PropertyDefinitionV1 {
                name: "amplitude".to_string(),
                label: "Amplitude".to_string(),
                ui: PropertyUiV1::Float {
                    min: 0.0,
                    max: 1_000.0,
                    step: 0.01,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                default: serde_json::json!(1.0),
            },
            PropertyDefinitionV1 {
                name: "seed".to_string(),
                label: "Seed".to_string(),
                ui: PropertyUiV1::Integer {
                    min: 0,
                    max: i64::MAX,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                default: serde_json::json!(0),
            },
        ],
        output_default: Some(PropertyValueV1::Number { value: 0.0 }),
    }
}

fn fill_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: FILL_COMPONENT_ID.to_string(),
        name: "Runtime Fill".to_string(),
        category: STYLE_CATEGORY.to_string(),
        group: "Style".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![STYLE_EVALUATE_V1.to_string()],
        properties: vec![
            color_property(255, 128, 32, 255),
            float_property(FloatPropertySpec {
                name: "offset",
                label: "Offset",
                min: -100.0,
                max: 100.0,
                step: 1.0,
                suffix: "px",
                min_hard_limit: false,
                max_hard_limit: false,
                default: 2.0,
            }),
        ],
        output_default: None,
    }
}

fn stroke_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: STROKE_COMPONENT_ID.to_string(),
        name: "Runtime Stroke".to_string(),
        category: STYLE_CATEGORY.to_string(),
        group: "Style".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![STYLE_EVALUATE_V1.to_string()],
        properties: vec![
            color_property(32, 128, 255, 255),
            float_property(FloatPropertySpec {
                name: "width",
                label: "Width",
                min: 0.0,
                max: 100.0,
                step: 0.5,
                suffix: "px",
                min_hard_limit: true,
                max_hard_limit: false,
                default: 3.0,
            }),
            float_property(FloatPropertySpec {
                name: "offset",
                label: "Offset",
                min: -100.0,
                max: 100.0,
                step: 1.0,
                suffix: "px",
                min_hard_limit: false,
                max_hard_limit: false,
                default: 0.0,
            }),
            dropdown_property("cap", "Cap", &["Round", "Square", "Butt"], "Round"),
            dropdown_property("join", "Join", &["Round", "Bevel", "Miter"], "Miter"),
            float_property(FloatPropertySpec {
                name: "miter",
                label: "Miter",
                min: 0.0,
                max: 100.0,
                step: 0.5,
                suffix: "",
                min_hard_limit: true,
                max_hard_limit: false,
                default: 4.0,
            }),
            PropertyDefinitionV1 {
                name: "dash_array".to_string(),
                label: "Dash Array".to_string(),
                ui: PropertyUiV1::Text,
                default: serde_json::json!("3 2"),
            },
            float_property(FloatPropertySpec {
                name: "dash_offset",
                label: "Dash Offset",
                min: -1_000.0,
                max: 1_000.0,
                step: 1.0,
                suffix: "px",
                min_hard_limit: false,
                max_hard_limit: false,
                default: 1.0,
            }),
        ],
        output_default: None,
    }
}

fn backplate_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: BACKPLATE_COMPONENT_ID.to_string(),
        name: "Runtime Backplate".to_string(),
        category: DECORATOR_CATEGORY.to_string(),
        group: "Decorator".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![DECORATOR_EVALUATE_V1.to_string()],
        properties: vec![
            dropdown_property("target", "Target", &["Block", "Line", "Char"], "Block"),
            dropdown_property(
                "shape",
                "Shape",
                &["Rect", "RoundedRect", "Circle"],
                "RoundedRect",
            ),
            color_property(0, 0, 0, 192),
            PropertyDefinitionV1 {
                name: "padding".to_string(),
                label: "Padding".to_string(),
                ui: PropertyUiV1::Vec4 {
                    suffix: "px".to_string(),
                },
                default: serde_json::json!({"x": 4.0, "y": 6.0, "z": 4.0, "w": 6.0}),
            },
            float_property(FloatPropertySpec {
                name: "corner_radius",
                label: "Corner Radius",
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix: "px",
                min_hard_limit: true,
                max_hard_limit: false,
                default: 3.0,
            }),
        ],
        output_default: None,
    }
}

fn color_property(r: u8, g: u8, b: u8, a: u8) -> PropertyDefinitionV1 {
    PropertyDefinitionV1 {
        name: "color".to_string(),
        label: "Color".to_string(),
        ui: PropertyUiV1::Color,
        default: serde_json::json!({"r": r, "g": g, "b": b, "a": a}),
    }
}

struct FloatPropertySpec<'a> {
    name: &'a str,
    label: &'a str,
    min: f64,
    max: f64,
    step: f64,
    suffix: &'a str,
    min_hard_limit: bool,
    max_hard_limit: bool,
    default: f64,
}

fn float_property(spec: FloatPropertySpec<'_>) -> PropertyDefinitionV1 {
    let FloatPropertySpec {
        name,
        label,
        min,
        max,
        step,
        suffix,
        min_hard_limit,
        max_hard_limit,
        default,
    } = spec;
    PropertyDefinitionV1 {
        name: name.to_string(),
        label: label.to_string(),
        ui: PropertyUiV1::Float {
            min,
            max,
            step,
            suffix: suffix.to_string(),
            min_hard_limit,
            max_hard_limit,
        },
        default: serde_json::json!(default),
    }
}

fn dropdown_property(
    name: &str,
    label: &str,
    options: &[&str],
    default: &str,
) -> PropertyDefinitionV1 {
    PropertyDefinitionV1 {
        name: name.to_string(),
        label: label.to_string(),
        ui: PropertyUiV1::Dropdown {
            options: options.iter().map(|option| (*option).to_string()).collect(),
        },
        default: serde_json::json!(default),
    }
}

fn ffi_guard(action: impl FnOnce() -> RuvieCallResult) -> RuvieCallResult {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result,
        Err(_) => RuvieCallResult::error(STATUS_PANIC, "plugin callback panicked"),
    }
}

unsafe extern "C" fn descriptor_json(_context: *mut c_void) -> RuvieCallResult {
    ffi_guard(|| {
        DESCRIPTOR_CALLS.fetch_add(1, Ordering::Relaxed);
        RuvieCallResult::ok_json(&descriptor())
    })
}

unsafe extern "C" fn invoke_json(
    _context: *mut c_void,
    request: RuvieBytesView,
) -> RuvieCallResult {
    ffi_guard(|| {
        if request.ptr.is_null() || request.len == 0 {
            return RuvieCallResult::error(STATUS_INVALID_REQUEST, "empty request");
        }
        // SAFETY: The ABI guarantees that the host-owned request remains valid
        // and immutable for exactly this callback invocation.
        let bytes = unsafe { std::slice::from_raw_parts(request.ptr, request.len) };
        let request: InvokeRequestV1 = match serde_json::from_slice(bytes) {
            Ok(request) => request,
            Err(error) => {
                return RuvieCallResult::error(STATUS_INVALID_REQUEST, error.to_string());
            }
        };
        let InvokeRequestV1 {
            component_id,
            category,
            operation,
            payload,
        } = request;
        match (category.as_str(), component_id.as_str(), operation.as_str()) {
            (PROPERTY_CATEGORY, COMPONENT_ID, DESCRIPTOR_CALLS_OPERATION) => {
                RuvieCallResult::ok_json(&serde_json::json!({
                "calls": DESCRIPTOR_CALLS.load(Ordering::Relaxed),
                }))
            }
            (PROPERTY_CATEGORY, COMPONENT_ID, PROPERTY_EVALUATE_V1) => evaluate_property(payload),
            (STYLE_CATEGORY, FILL_COMPONENT_ID, STYLE_EVALUATE_V1) => evaluate_fill(payload),
            (STYLE_CATEGORY, STROKE_COMPONENT_ID, STYLE_EVALUATE_V1) => evaluate_stroke(payload),
            (DECORATOR_CATEGORY, BACKPLATE_COMPONENT_ID, DECORATOR_EVALUATE_V1) => {
                evaluate_backplate(payload)
            }
            _ => RuvieCallResult::error(
                STATUS_INVALID_REQUEST,
                "unsupported component/category/operation",
            ),
        }
    })
}

fn evaluate_property(payload: serde_json::Value) -> RuvieCallResult {
    let payload: PropertyEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    if !has_exact_properties(&payload.properties, &["amplitude", "seed"]) {
        return invalid_request("property request does not match its descriptor");
    }
    let amplitude = match payload.properties.get("amplitude") {
        Some(PropertyValueV1::Number { value }) if value.is_finite() => value.abs(),
        _ => return invalid_request("amplitude must be a finite number"),
    };
    let seed = match payload.properties.get("seed") {
        Some(PropertyValueV1::Integer { value }) => u64::try_from(*value).unwrap_or_default(),
        _ => return invalid_request("seed must be an integer"),
    };
    let time_bucket = (payload.time * 1000.0).round() as u64;
    let mut rng = StdRng::seed_from_u64(seed ^ time_bucket);
    let value = rng.gen_range(-amplitude..=amplitude);
    RuvieCallResult::ok_json(&PropertyEvaluateResponseV1 {
        value: PropertyValueV1::Number { value },
    })
}

fn evaluate_fill(payload: serde_json::Value) -> RuvieCallResult {
    let payload: StyleEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    if !valid_config_metadata(payload.time, payload.fps)
        || !has_exact_properties(&payload.properties, &["color", "offset"])
    {
        return invalid_request("Fill request does not match its descriptor");
    }
    let Some(color) = property_color(&payload.properties, "color") else {
        return invalid_request("Fill color is invalid");
    };
    let Some(offset) = property_number(&payload.properties, "offset") else {
        return invalid_request("Fill offset is invalid");
    };
    if finite_f32(offset).is_none() || finite_f32(offset * 2.0).is_none() {
        return invalid_request("Fill offset is outside the renderer f32 contract");
    }
    RuvieCallResult::ok_json(&StyleOutputV1::Fill { color, offset })
}

fn evaluate_stroke(payload: serde_json::Value) -> RuvieCallResult {
    let payload: StyleEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    let expected = [
        "color",
        "width",
        "offset",
        "cap",
        "join",
        "miter",
        "dash_array",
        "dash_offset",
    ];
    if !valid_config_metadata(payload.time, payload.fps)
        || !has_exact_properties(&payload.properties, &expected)
    {
        return invalid_request("Stroke request does not match its descriptor");
    }
    let Some(color) = property_color(&payload.properties, "color") else {
        return invalid_request("Stroke color is invalid");
    };
    let Some(width) = property_number(&payload.properties, "width") else {
        return invalid_request("Stroke width is invalid");
    };
    if width < 0.0 {
        return invalid_request("Stroke width must be non-negative");
    }
    let Some(offset) = property_number(&payload.properties, "offset") else {
        return invalid_request("Stroke offset is invalid");
    };
    if !valid_stroke_geometry(width, offset) {
        return invalid_request("Stroke renderer-derived widths are unsafe");
    }
    let cap = match property_string(&payload.properties, "cap") {
        Some("Round") => StrokeCapV1::Round,
        Some("Square") => StrokeCapV1::Square,
        Some("Butt") => StrokeCapV1::Butt,
        _ => return invalid_request("Stroke cap is invalid"),
    };
    let join = match property_string(&payload.properties, "join") {
        Some("Round") => StrokeJoinV1::Round,
        Some("Bevel") => StrokeJoinV1::Bevel,
        Some("Miter") => StrokeJoinV1::Miter,
        _ => return invalid_request("Stroke join is invalid"),
    };
    let Some(miter) = property_number(&payload.properties, "miter") else {
        return invalid_request("Stroke miter is invalid");
    };
    if miter < 0.0 || finite_f32(miter).is_none() {
        return invalid_request("Stroke miter must be a non-negative f32");
    }
    let Some(dash_array) = property_string(&payload.properties, "dash_array").and_then(|value| {
        value
            .split_whitespace()
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()
    }) else {
        return invalid_request("Stroke dash array is invalid");
    };
    if !valid_dash_array(&dash_array) {
        return invalid_request("Stroke dash intervals violate the ABI work or period limits");
    }
    let Some(dash_offset) = property_number(&payload.properties, "dash_offset") else {
        return invalid_request("Stroke dash offset is invalid");
    };
    if finite_f32(dash_offset).is_none() {
        return invalid_request("Stroke dash offset is outside the f32 contract");
    }
    RuvieCallResult::ok_json(&StyleOutputV1::Stroke {
        color,
        width,
        offset,
        cap,
        join,
        miter,
        dash_array,
        dash_offset,
    })
}

fn valid_stroke_geometry(width: f64, offset: f64) -> bool {
    let finite_scalar = |value: f64| value.is_finite() && (value as f32).is_finite();
    if width < 0.0 || !finite_scalar(width) || !finite_scalar(offset) {
        return false;
    }
    if !finite_scalar((width + offset * 2.0).max(0.0)) {
        return false;
    }
    if width <= 0.0 || offset == 0.0 {
        return true;
    }
    let half_width = width / 2.0;
    let outer_radius = offset.abs() + half_width;
    let inner_radius = offset.abs() - half_width;
    finite_scalar(outer_radius * 2.0) && (inner_radius <= 0.0 || finite_scalar(inner_radius * 2.0))
}

fn valid_dash_array(values: &[f64]) -> bool {
    if values.is_empty() {
        return true;
    }
    if values.len() > MAX_STYLE_DASH_INTERVALS_V1 || !values.len().is_multiple_of(2) {
        return false;
    }
    let mut period = 0.0_f32;
    values.iter().all(|value| {
        let interval = *value as f32;
        if !value.is_finite() || !interval.is_finite() || interval <= 0.0 {
            return false;
        }
        period += interval;
        period.is_finite()
    }) && period > 0.0
}

fn evaluate_backplate(payload: serde_json::Value) -> RuvieCallResult {
    let payload: DecoratorEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    let expected = ["target", "shape", "color", "padding", "corner_radius"];
    if !valid_config_metadata(payload.time, payload.fps)
        || !has_exact_properties(&payload.properties, &expected)
    {
        return invalid_request("Backplate request does not match its descriptor");
    }
    let target = match property_string(&payload.properties, "target") {
        Some("Block") => DecoratorTargetV1::Block,
        Some("Line") => DecoratorTargetV1::Line,
        Some("Char") => DecoratorTargetV1::Char,
        _ => return invalid_request("Backplate target is invalid"),
    };
    let shape = match property_string(&payload.properties, "shape") {
        Some("Rect") => BackplateShapeV1::Rect,
        Some("RoundedRect") => BackplateShapeV1::RoundedRect,
        Some("Circle") => BackplateShapeV1::Circle,
        _ => return invalid_request("Backplate shape is invalid"),
    };
    let Some(color) = property_color(&payload.properties, "color") else {
        return invalid_request("Backplate color is invalid");
    };
    let padding = match payload.properties.get("padding") {
        Some(PropertyValueV1::Vec4 { x, y, z, w }) => match (
            finite_f32(*x),
            finite_f32(*y),
            finite_f32(*z),
            finite_f32(*w),
        ) {
            (Some(top), Some(right), Some(bottom), Some(left)) => InsetsV1 {
                top,
                right,
                bottom,
                left,
            },
            _ => return invalid_request("Backplate padding is outside the f32 contract"),
        },
        _ => return invalid_request("Backplate padding is invalid"),
    };
    let Some(corner_radius) = property_number(&payload.properties, "corner_radius") else {
        return invalid_request("Backplate corner radius is invalid");
    };
    let Some(corner_radius) = finite_f32(corner_radius).filter(|value| *value >= 0.0) else {
        return invalid_request("Backplate corner radius must be a non-negative f32");
    };
    if !valid_backplate_geometry(padding, corner_radius) {
        return invalid_request("Backplate renderer-derived geometry is unsafe");
    }
    RuvieCallResult::ok_json(&DecoratorOutputV1::Backplate {
        target,
        shape,
        color,
        padding,
        corner_radius,
    })
}

fn valid_backplate_geometry(padding: InsetsV1, corner_radius: f32) -> bool {
    let InsetsV1 {
        top,
        right,
        bottom,
        left,
    } = padding;
    let padded_left = -1.0_f32 - left;
    let padded_top = -2.0_f32 - top;
    let padded_right = 3.0_f32 + right;
    let padded_bottom = 4.0_f32 + bottom;
    [
        left + right,
        top + bottom,
        padded_left,
        padded_top,
        padded_right,
        padded_bottom,
        padded_right - padded_left,
        padded_bottom - padded_top,
        corner_radius * 2.0,
    ]
    .into_iter()
    .all(f32::is_finite)
}

fn valid_config_metadata(time: f64, fps: f64) -> bool {
    time.is_finite() && fps.is_finite() && fps > 0.0
}

fn finite_f32(value: f64) -> Option<f32> {
    let value = value as f32;
    value.is_finite().then_some(value)
}

fn has_exact_properties(
    properties: &std::collections::BTreeMap<String, PropertyValueV1>,
    expected: &[&str],
) -> bool {
    properties.len() == expected.len() && expected.iter().all(|name| properties.contains_key(*name))
}

fn property_number(
    properties: &std::collections::BTreeMap<String, PropertyValueV1>,
    name: &str,
) -> Option<f64> {
    match properties.get(name) {
        Some(PropertyValueV1::Number { value }) if value.is_finite() => Some(*value),
        _ => None,
    }
}

fn property_string<'a>(
    properties: &'a std::collections::BTreeMap<String, PropertyValueV1>,
    name: &str,
) -> Option<&'a str> {
    match properties.get(name) {
        Some(PropertyValueV1::String { value }) => Some(value),
        _ => None,
    }
}

fn property_color(
    properties: &std::collections::BTreeMap<String, PropertyValueV1>,
    name: &str,
) -> Option<ColorV1> {
    match properties.get(name) {
        Some(PropertyValueV1::Color { r, g, b, a }) => Some(ColorV1 {
            r: *r,
            g: *g,
            b: *b,
            a: *a,
        }),
        _ => None,
    }
}

fn invalid_request(detail: impl std::fmt::Display) -> RuvieCallResult {
    RuvieCallResult::error(STATUS_INVALID_REQUEST, detail.to_string())
}

fn extension_guard(action: impl FnOnce() -> RuvieExtensionResultV1) -> RuvieExtensionResultV1 {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result,
        Err(_) => RuvieExtensionResultV1::error(STATUS_PANIC, "plugin callback panicked"),
    }
}

unsafe fn bytes_from_view<'a>(view: RuvieBytesView) -> Result<&'a [u8], &'static str> {
    if view.len == 0 {
        return Ok(&[]);
    }
    if view.ptr.is_null() {
        return Err("non-empty byte view has a null pointer");
    }
    // SAFETY: The caller is inside the ABI callback for which the host keeps
    // this immutable borrowed byte view alive.
    Ok(unsafe { std::slice::from_raw_parts(view.ptr, view.len) })
}

unsafe fn utf8_from_view<'a>(view: RuvieBytesView) -> Result<&'a str, &'static str> {
    // SAFETY: The same callback-scoped borrowed-view contract applies.
    std::str::from_utf8(unsafe { bytes_from_view(view)? }).map_err(|_| "byte view is not UTF-8")
}

fn invalid_extension(detail: impl Into<String>) -> RuvieExtensionResultV1 {
    RuvieExtensionResultV1::error(STATUS_INVALID_REQUEST, detail)
}

unsafe extern "C" fn effect_create_instance(
    _context: *mut c_void,
    component_id: RuvieBytesView,
    properties: RuviePropertyMapViewV1,
    out_instance: *mut u64,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        if out_instance.is_null() {
            return invalid_extension("Effect out_instance is null");
        }
        // SAFETY: Component IDs are callback-scoped host byte views.
        let component_id = match unsafe { utf8_from_view(component_id) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if component_id != EFFECT_COMPONENT_ID {
            return RuvieExtensionResultV1::unsupported();
        }
        if properties.len != 1 || properties.ptr.is_null() {
            return invalid_extension("Effect requires exactly one typed property");
        }
        // SAFETY: The host provides `len` initialized property views for this
        // callback and does not mutate them concurrently.
        let properties = unsafe { std::slice::from_raw_parts(properties.ptr, properties.len) };
        let property = &properties[0];
        // SAFETY: Property names are callback-scoped host byte views.
        let name = match unsafe { utf8_from_view(property.name) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if name != "red"
            || property.value_type != PROPERTY_VALUE_INTEGER_V1
            || !(0..=255).contains(&property.integer)
        {
            return invalid_extension("Effect red must be an integer in 0..=255");
        }
        // Zero is reserved by the host, so encode red as one through 256.
        // SAFETY: The pointer was checked above and is uniquely writable for
        // this callback.
        unsafe { *out_instance = u64::try_from(property.integer).unwrap_or_default() + 1 };
        RuvieExtensionResultV1::ok()
    })
}

unsafe extern "C" fn effect_process(
    _context: *mut c_void,
    instance: u64,
    time_seconds: f64,
    input: *const RuvieRgba8FrameViewV1,
    output: *mut RuvieOwnedRgba8FrameV1,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        if input.is_null() || output.is_null() || !(1..=256).contains(&instance) {
            return invalid_extension("Effect frame pointer or instance is invalid");
        }
        if !time_seconds.is_finite() {
            return invalid_extension("Effect time must be finite");
        }
        // SAFETY: Both pointers are host-owned and valid for the callback.
        let input = unsafe { &*input };
        if input.struct_size < std::mem::size_of::<RuvieRgba8FrameViewV1>()
            || input.alpha_mode != ALPHA_MODE_STRAIGHT_V1
            || input.color_profile != COLOR_PROFILE_SRGB_V1
            || input.width == 0
            || input.height == 0
            || input.width > MAX_CPU_RGBA8_DIMENSION_V1
            || input.height > MAX_CPU_RGBA8_DIMENSION_V1
        {
            return invalid_extension("Effect input frame metadata is invalid");
        }
        let row_bytes = match usize::try_from(input.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
        {
            Some(value) => value,
            None => return invalid_extension("Effect input row-byte overflow"),
        };
        let expected = match input
            .stride_bytes
            .checked_mul(usize::try_from(input.height).unwrap_or(usize::MAX))
        {
            Some(value) => value,
            None => return invalid_extension("Effect input frame-byte overflow"),
        };
        if input.stride_bytes < row_bytes
            || expected != input.pixels.len
            || expected > MAX_CPU_RGBA8_FRAME_BYTES_V1
        {
            return invalid_extension("Effect input stride or length is invalid");
        }
        // SAFETY: The complete input layout and non-null pointer contract are
        // checked by `bytes_from_view` before iteration.
        let pixels = match unsafe { bytes_from_view(input.pixels) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        let red = u8::try_from(instance - 1).unwrap_or_default();
        let mut tinted = Vec::with_capacity(pixels.len());
        for row in pixels.chunks_exact(input.stride_bytes) {
            for pixel in row[..row_bytes].chunks_exact(4) {
                if pixel[3] == 0 {
                    tinted.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    tinted.extend_from_slice(&[red, 0, 0, pixel[3]]);
                }
            }
            tinted.resize(tinted.len() + (input.stride_bytes - row_bytes), 0);
        }
        let frame = RuvieOwnedRgba8FrameV1 {
            struct_size: std::mem::size_of::<RuvieOwnedRgba8FrameV1>(),
            width: input.width,
            height: input.height,
            stride_bytes: input.stride_bytes,
            alpha_mode: ALPHA_MODE_STRAIGHT_V1,
            color_profile: COLOR_PROFILE_SRGB_V1,
            pixels: RuvieBuffer::from_vec(tinted),
        };
        // SAFETY: The output pointer is uniquely writable for this callback.
        unsafe { *output = frame };
        RuvieExtensionResultV1::ok()
    })
}

unsafe extern "C" fn effect_release_instance(_context: *mut c_void, _instance: u64) {}

const IMAGE_FIXTURE_MAGIC: &[u8; 8] = b"RUVRGBA1";
const VIDEO_FIXTURE_MAGIC: &[u8; 8] = b"RUVVID01";
const IMAGE_FIXTURE_SUFFIX: &str = ".rgba-fixture";
const VIDEO_FIXTURE_SUFFIX: &str = ".rgba-video-fixture";
const VIDEO_DURATION_SECONDS: f64 = 2.0;
const VIDEO_FPS: f64 = 24.0;
const VIDEO_FRAME_COUNT: u64 = 48;

enum FixtureRequest {
    Image,
    Video {
        source_time: f64,
        stream_index: u32,
        input_color_space: String,
        output_color_space: String,
    },
}

struct RgbaFixture {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    request: FixtureRequest,
}

fn fixture_path_is_supported(path: &str) -> bool {
    path.ends_with(IMAGE_FIXTURE_SUFFIX) || path.ends_with(VIDEO_FIXTURE_SUFFIX)
}

fn read_rgba_fixture(path: &str) -> Result<RgbaFixture, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("could not read fixture: {error}"))?;
    if bytes.len() < 16 {
        return Err("fixture header magic is invalid".to_string());
    }
    let width = u32::from_le_bytes(bytes[8..12].try_into().unwrap_or_default());
    let height = u32::from_le_bytes(bytes[12..16].try_into().unwrap_or_default());
    if width == 0
        || height == 0
        || width > MAX_CPU_RGBA8_DIMENSION_V1
        || height > MAX_CPU_RGBA8_DIMENSION_V1
    {
        return Err(format!("fixture dimensions {width}x{height} are invalid"));
    }
    let expected_pixels = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .and_then(|row| row.checked_mul(usize::try_from(height).unwrap_or(usize::MAX)))
        .ok_or_else(|| "fixture pixel length overflow".to_string())?;
    if expected_pixels > MAX_CPU_RGBA8_FRAME_BYTES_V1 {
        return Err("fixture pixel length exceeds the ABI limit".to_string());
    }

    let (request, pixels_offset) = if &bytes[..8] == IMAGE_FIXTURE_MAGIC {
        (FixtureRequest::Image, 16)
    } else if &bytes[..8] == VIDEO_FIXTURE_MAGIC {
        if bytes.len() < 32 {
            return Err("video fixture request header is truncated".to_string());
        }
        let source_time = f64::from_le_bytes(bytes[16..24].try_into().unwrap_or_default());
        if !source_time.is_finite() || source_time < 0.0 {
            return Err("video fixture source time is invalid".to_string());
        }
        let stream_index = u32::from_le_bytes(bytes[24..28].try_into().unwrap_or_default());
        let input_len = usize::from(u16::from_le_bytes(
            bytes[28..30].try_into().unwrap_or_default(),
        ));
        let output_len = usize::from(u16::from_le_bytes(
            bytes[30..32].try_into().unwrap_or_default(),
        ));
        let input_end = 32_usize
            .checked_add(input_len)
            .ok_or_else(|| "video fixture request header length overflow".to_string())?;
        let output_end = input_end
            .checked_add(output_len)
            .ok_or_else(|| "video fixture request header length overflow".to_string())?;
        if bytes.len() < output_end {
            return Err("video fixture request header is truncated".to_string());
        }
        let input_color_space = std::str::from_utf8(&bytes[32..input_end])
            .map_err(|error| format!("video fixture input color space is invalid: {error}"))?
            .to_string();
        let output_color_space = std::str::from_utf8(&bytes[input_end..output_end])
            .map_err(|error| format!("video fixture output color space is invalid: {error}"))?
            .to_string();
        (
            FixtureRequest::Video {
                source_time,
                stream_index,
                input_color_space,
                output_color_space,
            },
            output_end,
        )
    } else {
        return Err("fixture header magic is invalid".to_string());
    };
    let expected_len = pixels_offset
        .checked_add(expected_pixels)
        .ok_or_else(|| "fixture payload length overflow".to_string())?;
    if bytes.len() != expected_len {
        return Err(format!(
            "fixture payload length {} does not match expected {expected_pixels}",
            bytes.len().saturating_sub(pixels_offset)
        ));
    }
    Ok(RgbaFixture {
        width,
        height,
        pixels: bytes[pixels_offset..].to_vec(),
        request,
    })
}

unsafe extern "C" fn loader_open(
    _context: *mut c_void,
    component_id: RuvieBytesView,
    path: RuvieBytesView,
    metadata: *mut RuvieAssetMetadataV1,
    metadata_capacity: usize,
    out_metadata_len: *mut usize,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        // SAFETY: IDs and paths are callback-scoped host byte views.
        let component_id = match unsafe { utf8_from_view(component_id) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        // SAFETY: Same borrowed-view contract as above.
        let path = match unsafe { utf8_from_view(path) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if component_id != LOADER_COMPONENT_ID || !fixture_path_is_supported(path) {
            return RuvieExtensionResultV1::unsupported();
        }
        if metadata.is_null() || out_metadata_len.is_null() || metadata_capacity < 1 {
            return invalid_extension("Loader metadata output is invalid");
        }
        let fixture = match read_rgba_fixture(path) {
            Ok(value) => value,
            Err(error) => return RuvieExtensionResultV1::error(STATUS_PLUGIN_ERROR, error),
        };
        let value = match &fixture.request {
            FixtureRequest::Image if path.ends_with(IMAGE_FIXTURE_SUFFIX) => RuvieAssetMetadataV1 {
                kind: ASSET_KIND_IMAGE_V1,
                present_fields: ASSET_METADATA_DIMENSIONS_V1,
                width: fixture.width,
                height: fixture.height,
                ..RuvieAssetMetadataV1::default()
            },
            FixtureRequest::Video { stream_index, .. } if path.ends_with(VIDEO_FIXTURE_SUFFIX) => {
                RuvieAssetMetadataV1 {
                    kind: ASSET_KIND_VIDEO_V1,
                    present_fields: ASSET_METADATA_DURATION_V1
                        | ASSET_METADATA_FPS_V1
                        | ASSET_METADATA_DIMENSIONS_V1
                        | ASSET_METADATA_STREAM_INDEX_V1
                        | ASSET_METADATA_FRAME_COUNT_V1
                        | ASSET_METADATA_TIME_BASE_V1,
                    duration_seconds: VIDEO_DURATION_SECONDS,
                    fps: VIDEO_FPS,
                    width: fixture.width,
                    height: fixture.height,
                    stream_index: *stream_index,
                    frame_count: VIDEO_FRAME_COUNT,
                    time_base_numerator: 1,
                    time_base_denominator: VIDEO_FPS as i32,
                }
            }
            FixtureRequest::Image | FixtureRequest::Video { .. } => {
                return RuvieExtensionResultV1::error(
                    STATUS_PLUGIN_ERROR,
                    "fixture magic does not match its path suffix",
                );
            }
        };
        // SAFETY: Capacity is at least one and both output pointers were
        // checked. The host initialized and owns this memory.
        unsafe {
            *metadata = value;
            *out_metadata_len = 1;
        }
        RuvieExtensionResultV1::ok()
    })
}

unsafe extern "C" fn loader_load(
    _context: *mut c_void,
    component_id: RuvieBytesView,
    request: *const RuvieLoaderRequestV1,
    output: *mut RuvieOwnedRgba8FrameV1,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        if request.is_null() || output.is_null() {
            return invalid_extension("Loader request or output is null");
        }
        // SAFETY: Component IDs are callback-scoped host byte views.
        let component_id = match unsafe { utf8_from_view(component_id) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        // SAFETY: The request pointer is host-owned for this callback.
        let request = unsafe { &*request };
        if request.struct_size < std::mem::size_of::<RuvieLoaderRequestV1>() {
            return invalid_extension("Loader request table is truncated");
        }
        // SAFETY: The request path is callback-scoped host memory.
        let path = match unsafe { utf8_from_view(request.path) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if component_id != LOADER_COMPONENT_ID || !fixture_path_is_supported(path) {
            return RuvieExtensionResultV1::unsupported();
        }
        let fixture = match read_rgba_fixture(path) {
            Ok(value) => value,
            Err(error) => return RuvieExtensionResultV1::error(STATUS_PLUGIN_ERROR, error),
        };
        match &fixture.request {
            FixtureRequest::Image
                if request.request_kind == LOAD_REQUEST_IMAGE_V1
                    && path.ends_with(IMAGE_FIXTURE_SUFFIX) => {}
            FixtureRequest::Video {
                source_time,
                stream_index,
                input_color_space,
                output_color_space,
            } if request.request_kind == LOAD_REQUEST_VIDEO_FRAME_V1
                && path.ends_with(VIDEO_FIXTURE_SUFFIX) =>
            {
                // SAFETY: Color-space names are callback-scoped host views.
                let input = match unsafe { utf8_from_view(request.input_color_space) } {
                    Ok(value) => value,
                    Err(error) => return invalid_extension(error),
                };
                // SAFETY: Same borrowed-view contract as above.
                let output = match unsafe { utf8_from_view(request.output_color_space) } {
                    Ok(value) => value,
                    Err(error) => return invalid_extension(error),
                };
                if request.source_time.to_bits() != source_time.to_bits()
                    || request.has_stream_index != 1
                    || request.stream_index != *stream_index
                    || input != input_color_space
                    || output != output_color_space
                {
                    return RuvieExtensionResultV1::error(
                        STATUS_PLUGIN_ERROR,
                        format!(
                            "video request metadata mismatch: time={}, stream={:?}, input={input:?}, output={output:?}",
                            request.source_time,
                            (request.has_stream_index == 1).then_some(request.stream_index),
                        ),
                    );
                }
            }
            FixtureRequest::Image | FixtureRequest::Video { .. } => {
                return RuvieExtensionResultV1::unsupported();
            }
        }
        let stride_bytes = usize::try_from(fixture.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .unwrap_or_default();
        // SAFETY: Output is uniquely writable and starts in the host-defined
        // empty ownership state.
        unsafe {
            *output = RuvieOwnedRgba8FrameV1 {
                struct_size: std::mem::size_of::<RuvieOwnedRgba8FrameV1>(),
                width: fixture.width,
                height: fixture.height,
                stride_bytes,
                alpha_mode: ALPHA_MODE_STRAIGHT_V1,
                color_profile: COLOR_PROFILE_SRGB_V1,
                pixels: RuvieBuffer::from_vec(fixture.pixels),
            }
        };
        RuvieExtensionResultV1::ok()
    })
}

unsafe extern "C" fn free_frame(_context: *mut c_void, frame: RuvieOwnedRgba8FrameV1) {
    // SAFETY: The host returns a structurally reclaimable frame exactly once
    // to the extension table that allocated it.
    unsafe { ruvie_plugin_api::free_owned_buffer(frame.pixels) };
}

unsafe extern "C" fn query_extension(
    _context: *mut c_void,
    extension_name: RuvieBytesView,
) -> *const c_void {
    // This callback cannot report an error, so invalid views simply decline.
    // SAFETY: `extension_name` is borrowed only for this callback invocation;
    // `bytes_from_view` validates the pointer/length pair before exposing it.
    let Ok(name) = (unsafe { bytes_from_view(extension_name) }) else {
        return std::ptr::null();
    };
    if name == EFFECT_CPU_RGBA8_EXTENSION_V1.as_bytes() {
        (&EFFECT_API as *const RuvieEffectCpuRgba8ApiV1).cast()
    } else if name == LOADER_CPU_RGBA8_EXTENSION_V1.as_bytes() {
        (&LOADER_API as *const RuvieLoaderCpuRgba8ApiV1).cast()
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn free_buffer(_context: *mut c_void, buffer: RuvieBuffer) {
    // SAFETY: The host returns every plugin-owned buffer exactly once to the
    // same dynamic library that allocated it.
    unsafe { ruvie_plugin_api::free_owned_buffer(buffer) };
}

static API: RuviePluginApiV1 = RuviePluginApiV1 {
    abi_version: RUVIE_PLUGIN_ABI_V1,
    struct_size: std::mem::size_of::<RuviePluginApiV1>(),
    context: std::ptr::null_mut(),
    descriptor_json: Some(descriptor_json),
    invoke_json: Some(invoke_json),
    free_buffer: Some(free_buffer),
    query_extension: Some(query_extension),
};

static EFFECT_API: RuvieEffectCpuRgba8ApiV1 = RuvieEffectCpuRgba8ApiV1 {
    abi_version: RUVIE_PLUGIN_ABI_V1,
    struct_size: std::mem::size_of::<RuvieEffectCpuRgba8ApiV1>(),
    context: std::ptr::null_mut(),
    create_instance: Some(effect_create_instance),
    process: Some(effect_process),
    release_instance: Some(effect_release_instance),
    free_frame: Some(free_frame),
};

static LOADER_API: RuvieLoaderCpuRgba8ApiV1 = RuvieLoaderCpuRgba8ApiV1 {
    abi_version: RUVIE_PLUGIN_ABI_V1,
    struct_size: std::mem::size_of::<RuvieLoaderCpuRgba8ApiV1>(),
    context: std::ptr::null_mut(),
    open: Some(loader_open),
    load: Some(loader_load),
    free_frame: Some(free_frame),
};

#[unsafe(no_mangle)]
pub extern "C" fn ruvie_plugin_entry_v1() -> *const RuviePluginApiV1 {
    &API
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component<'a>(descriptor: &'a PluginDescriptorV1, id: &str) -> &'a ComponentDescriptorV1 {
        descriptor
            .components
            .iter()
            .find(|component| component.id == id)
            .expect("test component is declared")
    }

    fn float_ui<'a>(component: &'a ComponentDescriptorV1, name: &str) -> &'a PropertyUiV1 {
        &component
            .properties
            .iter()
            .find(|property| property.name == name)
            .expect("test property is declared")
            .ui
    }

    #[test]
    fn evaluator_is_deterministic_and_respects_amplitude() {
        let payload = PropertyEvaluateRequestV1 {
            time: 1.25,
            fps: 30.0,
            properties: std::collections::BTreeMap::from([
                (
                    "amplitude".to_string(),
                    PropertyValueV1::Number { value: 3.0 },
                ),
                ("seed".to_string(), PropertyValueV1::Integer { value: 42 }),
            ]),
        };
        let evaluate = || {
            let amplitude = match payload.properties.get("amplitude") {
                Some(PropertyValueV1::Number { value }) => value.abs(),
                _ => 1.0,
            };
            let seed = match payload.properties.get("seed") {
                Some(PropertyValueV1::Integer { value }) => {
                    u64::try_from(*value).unwrap_or_default()
                }
                _ => 0,
            };
            let bucket = (payload.time * 1000.0).round() as u64;
            StdRng::seed_from_u64(seed ^ bucket).gen_range(-amplitude..=amplitude)
        };
        let first = evaluate();
        assert_eq!(first, evaluate());
        assert!((-3.0..=3.0).contains(&first));
    }

    #[test]
    fn config_descriptor_metadata_matches_runtime_safety_contracts() {
        let descriptor = descriptor();
        let stroke = component(&descriptor, STROKE_COMPONENT_ID);
        let backplate = component(&descriptor, BACKPLATE_COMPONENT_ID);
        for (component, name) in [
            (stroke, "width"),
            (stroke, "miter"),
            (backplate, "corner_radius"),
        ] {
            assert!(matches!(
                float_ui(component, name),
                PropertyUiV1::Float {
                    min: 0.0,
                    min_hard_limit: true,
                    ..
                }
            ));
        }
        for name in ["offset", "dash_offset"] {
            assert!(matches!(
                float_ui(stroke, name),
                PropertyUiV1::Float {
                    min_hard_limit: false,
                    ..
                }
            ));
        }
        let backplate_target = backplate
            .properties
            .iter()
            .find(|property| property.name == "target")
            .expect("Backplate target is declared");
        let PropertyUiV1::Dropdown { options } = &backplate_target.ui else {
            panic!("Backplate target must be a dropdown")
        };
        assert_eq!(options, &["Block", "Line", "Char"]);
        assert!(!options.iter().any(|option| option == "Parts"));
    }

    #[test]
    fn stroke_fixture_honors_the_abi_dash_work_and_period_limits() {
        assert!(valid_dash_array(&[]));
        assert!(valid_dash_array(&vec![1.0; MAX_STYLE_DASH_INTERVALS_V1]));
        assert!(!valid_dash_array(&[f32::MAX as f64, f32::MAX as f64]));
        assert!(!valid_dash_array(&vec![
            1.0;
            MAX_STYLE_DASH_INTERVALS_V1 + 2
        ]));
    }

    #[test]
    fn config_fixture_rejects_renderer_derived_overflow() {
        assert!(!valid_stroke_geometry(1.0, -(f32::MAX as f64)));
        assert!(valid_stroke_geometry(1.0, -(f32::MAX as f64) / 4.0));
        assert!(!valid_backplate_geometry(
            InsetsV1 {
                top: 0.0,
                right: f32::MAX,
                bottom: 0.0,
                left: f32::MAX,
            },
            0.0,
        ));
        assert!(valid_backplate_geometry(
            InsetsV1 {
                top: -1.0,
                right: 2.0,
                bottom: -1.0,
                left: 2.0,
            },
            1.0,
        ));
    }
}
