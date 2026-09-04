use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static ENABLED: AtomicBool = AtomicBool::new(false);
static REGISTRY: Lazy<ComponentRegistry> = Lazy::new(ComponentRegistry::default);

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct QaRect {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub width: f32,
    pub height: f32,
    pub center_x: f32,
    pub center_y: f32,
}

impl QaRect {
    fn from_egui(rect: egui::Rect) -> Self {
        Self {
            min_x: rect.min.x,
            min_y: rect.min.y,
            max_x: rect.max.x,
            max_y: rect.max.y,
            width: rect.width(),
            height: rect.height(),
            center_x: rect.center().x,
            center_y: rect.center().y,
        }
    }

    fn scaled(self, scale: f32) -> Self {
        Self {
            min_x: self.min_x * scale,
            min_y: self.min_y * scale,
            max_x: self.max_x * scale,
            max_y: self.max_y * scale,
            width: self.width * scale,
            height: self.height * scale,
            center_x: self.center_x * scale,
            center_y: self.center_y * scale,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComponentInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub component_type: String,
    pub rect_points: QaRect,
    pub rect_pixels: QaRect,
    pub enabled: bool,
    pub visible: bool,
    pub frame: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FrameSnapshot {
    pub frame: u64,
    pub pixels_per_point: f32,
    pub screen_rect_points: QaRect,
    pub components: Vec<ComponentInfo>,
}

impl Default for FrameSnapshot {
    fn default() -> Self {
        Self {
            frame: 0,
            pixels_per_point: 1.0,
            screen_rect_points: QaRect::default(),
            components: Vec::new(),
        }
    }
}

#[derive(Default)]
struct RegistryState {
    next_frame: u64,
    building: Option<BuildingFrame>,
    published: FrameSnapshot,
}

struct BuildingFrame {
    frame: u64,
    pixels_per_point: f32,
    screen_rect_points: QaRect,
    screen_rect: egui::Rect,
    components: BTreeMap<String, ComponentInfo>,
}

#[derive(Default)]
pub struct ComponentRegistry {
    state: Mutex<RegistryState>,
}

impl ComponentRegistry {
    pub fn begin_frame(&self, pixels_per_point: f32, screen_rect: egui::Rect) {
        let pixels_per_point = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            pixels_per_point
        } else {
            1.0
        };
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.next_frame = state.next_frame.saturating_add(1);
        state.building = Some(BuildingFrame {
            frame: state.next_frame,
            pixels_per_point,
            screen_rect_points: QaRect::from_egui(screen_rect),
            screen_rect,
            components: BTreeMap::new(),
        });
    }

    pub fn register(
        &self,
        id: impl Into<String>,
        component_type: impl Into<String>,
        rect: egui::Rect,
        enabled: bool,
        metadata: Option<Value>,
    ) {
        if !rect.min.x.is_finite()
            || !rect.min.y.is_finite()
            || !rect.max.x.is_finite()
            || !rect.max.y.is_finite()
        {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(building) = state.building.as_mut() else {
            return;
        };
        let id = id.into();
        let rect_points = QaRect::from_egui(rect);
        let component = ComponentInfo {
            id: id.clone(),
            component_type: component_type.into(),
            rect_points,
            rect_pixels: rect_points.scaled(building.pixels_per_point),
            enabled,
            visible: rect.is_positive() && building.screen_rect.intersects(rect),
            frame: building.frame,
            metadata,
        };
        building.components.insert(id, component);
    }

    pub fn end_frame(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(building) = state.building.take() else {
            return;
        };
        state.published = FrameSnapshot {
            frame: building.frame,
            pixels_per_point: building.pixels_per_point,
            screen_rect_points: building.screen_rect_points,
            components: building.components.into_values().collect(),
        };
    }

    pub fn snapshot(&self) -> FrameSnapshot {
        self.state
            .lock()
            .map(|state| state.published.clone())
            .unwrap_or_default()
    }

    pub fn component(&self, id: &str) -> Option<ComponentInfo> {
        self.state.lock().ok().and_then(|state| {
            state
                .published
                .components
                .iter()
                .find(|component| component.id == id)
                .cloned()
        })
    }
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

pub fn begin_frame(ctx: &egui::Context) {
    if !is_enabled() {
        return;
    }
    let screen_rect = ctx.input(|input| input.content_rect());
    REGISTRY.begin_frame(ctx.pixels_per_point(), screen_rect);
}

pub fn register_component(
    id: impl Into<String>,
    component_type: impl Into<String>,
    rect: egui::Rect,
) {
    register_component_with_metadata(id, component_type, rect, true, None);
}

pub fn register_component_with_metadata(
    id: impl Into<String>,
    component_type: impl Into<String>,
    rect: egui::Rect,
    enabled: bool,
    metadata: Option<Value>,
) {
    if !is_enabled() {
        return;
    }
    REGISTRY.register(id, component_type, rect, enabled, metadata);
}

pub fn end_frame() {
    if is_enabled() {
        REGISTRY.end_frame();
    }
}

pub fn snapshot() -> FrameSnapshot {
    REGISTRY.snapshot()
}

pub fn component(id: &str) -> Option<ComponentInfo> {
    REGISTRY.component(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_publishes_complete_frames_and_replaces_stale_components() {
        let registry = ComponentRegistry::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 80.0));

        registry.begin_frame(2.0, screen);
        registry.register(
            "node:one",
            "node",
            egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(30.0, 40.0)),
            true,
            None,
        );
        assert!(registry.snapshot().components.is_empty());

        registry.end_frame();
        let first = registry.snapshot();
        assert_eq!(first.frame, 1);
        assert_eq!(first.components[0].rect_points.center_x, 20.0);
        assert_eq!(first.components[0].rect_pixels.center_x, 40.0);

        registry.begin_frame(2.0, screen);
        registry.register(
            "preview.canvas",
            "preview",
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
            true,
            None,
        );
        registry.end_frame();

        let second = registry.snapshot();
        assert_eq!(second.frame, 2);
        assert!(second.components.iter().all(|item| item.id != "node:one"));
        assert_eq!(registry.component("preview.canvas").unwrap().frame, 2);
    }
}
