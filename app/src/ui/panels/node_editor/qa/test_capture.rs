use eframe::egui;
use std::collections::HashMap;

#[cfg(test)]
thread_local! {
    static TEST_RENDER_RECTS: std::cell::RefCell<HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(HashMap::new());
    static TEST_RENDER_METADATA: std::cell::RefCell<HashMap<String, serde_json::Value>> =
        std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn capture_test_rect(id: &str, rect: egui::Rect) {
    TEST_RENDER_RECTS.with(|rects| {
        rects.borrow_mut().insert(id.to_string(), rect);
    });
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn capture_test_metadata(
    id: &str,
    metadata: &serde_json::Value,
) {
    TEST_RENDER_METADATA.with(|entries| {
        entries
            .borrow_mut()
            .insert(id.to_string(), metadata.clone());
    });
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn reset_test_rects() {
    TEST_RENDER_RECTS.with(|rects| rects.borrow_mut().clear());
    TEST_RENDER_METADATA.with(|entries| entries.borrow_mut().clear());
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn test_rect(id: &str) -> Option<egui::Rect> {
    TEST_RENDER_RECTS.with(|rects| rects.borrow().get(id).copied())
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn test_rects() -> HashMap<String, egui::Rect> {
    TEST_RENDER_RECTS.with(|rects| rects.borrow().clone())
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn test_metadata(id: &str) -> Option<serde_json::Value> {
    TEST_RENDER_METADATA.with(|entries| entries.borrow().get(id).cloned())
}
