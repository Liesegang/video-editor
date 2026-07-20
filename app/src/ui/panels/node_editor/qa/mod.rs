#[cfg(test)]
mod test_capture;

#[cfg(test)]
pub(in crate::ui::panels::node_editor) use test_capture::{
    capture_test_metadata, capture_test_rect, reset_test_rects, test_metadata, test_rect,
    test_rects,
};
