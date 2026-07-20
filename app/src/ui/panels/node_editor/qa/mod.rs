mod components;

pub(in crate::ui::panels::node_editor) use components::{
    clipped_qa_rect, edge_endpoint_qa_metadata, qa_container_key, qa_port_id, qa_rect_metadata,
    wire_port_drop_rect,
};

#[cfg(test)]
mod test_capture;

#[cfg(test)]
pub(in crate::ui::panels::node_editor) use test_capture::{
    capture_test_metadata, capture_test_rect, reset_test_rects, test_metadata, test_rect,
    test_rects,
};
