//! Image chain resolution — follows connections from clip output through
//! fill/stroke, transform, and effect nodes.

use uuid::Uuid;

use super::EvalEngine;
use crate::error::LibraryError;
use crate::evaluation::context::EvalContext;
use crate::project::clip::TrackClipKind;
use crate::rendering::renderer::RenderOutput;

impl EvalEngine {
    /// Resolve the full image chain for a clip.
    ///
    /// For text/shape clips: starts from `shape_out`, follows through fill/stroke
    /// nodes (which produce images), then through transform/effect nodes.
    /// For other clips: starts from `image_out` and follows through transform/effects.
    pub(crate) fn resolve_image_chain(
        &self,
        clip_id: Uuid,
        clip_kind: &TrackClipKind,
        ctx: &mut EvalContext,
    ) -> Result<Option<RenderOutput>, LibraryError> {
        // Determine the clip's primary output pin
        let primary_pin = match clip_kind {
            TrackClipKind::Text | TrackClipKind::Shape => "shape_out",
            _ => "image_out",
        };

        // Evaluate the clip's primary output
        let clip_output = ctx.evaluate_pin(clip_id, primary_pin)?;

        // For shape_out: the clip produces shape data, not an image.
        // Follow the shape chain (clip → effector/decorator → fill/stroke)
        // until we find a style node that produces image_out.
        if primary_pin == "shape_out" {
            // Trigger shape evaluation (ensures it's cached)
            let _ = clip_output;

            // Follow shape_out → shape_in connections until we hit a style node
            let mut current_id = clip_id;
            let mut current_out_pin = "shape_out";

            let terminal_node_id = loop {
                let downstream = ctx.find_downstream(current_id, current_out_pin);
                let next = downstream.iter().find(|(_, pin)| pin == "shape_in");
                match next {
                    Some((next_id, _)) => {
                        let next_id = *next_id;
                        let is_style = ctx
                            .project
                            .get_graph_node(next_id)
                            .map(|g| g.type_id.starts_with("style."))
                            .unwrap_or(false);
                        if is_style {
                            break Some(next_id);
                        }
                        // Effector/decorator — continue following shape chain
                        current_id = next_id;
                        current_out_pin = "shape_out";
                    }
                    None => break None,
                }
            };

            let fill_id = match terminal_node_id {
                Some(id) => id,
                None => return Ok(None),
            };

            // Evaluate fill/stroke's image_out (pulls shape_in recursively)
            let fill_output = ctx.evaluate_pin(fill_id, "image_out")?;
            let mut current_image = match fill_output.into_image() {
                Some(img) => img,
                None => return Ok(None),
            };

            // Continue following image_out → image_in chain (transform, effects)
            let mut current_node_id = fill_id;
            loop {
                let downstream = ctx.find_downstream(current_node_id, "image_out");
                let next = downstream.iter().find(|(_, pin)| pin == "image_in");
                match next {
                    Some((next_node_id, _)) => {
                        let next_id = *next_node_id;
                        let next_output = ctx.evaluate_pin(next_id, "image_out")?;
                        match next_output.into_image() {
                            Some(img) => {
                                current_image = img;
                                current_node_id = next_id;
                            }
                            None => break,
                        }
                    }
                    None => break,
                }
            }

            return Ok(Some(current_image));
        }

        // For image_out: existing behavior
        let mut current_image = match clip_output.into_image() {
            Some(img) => img,
            None => return Ok(None),
        };

        // Follow the image chain: clip.image_out → next_node.image_in → next_node.image_out → ...
        let mut current_node_id = clip_id;

        loop {
            let downstream = ctx.find_downstream(current_node_id, "image_out");
            let next = downstream.iter().find(|(_, pin)| pin == "image_in");

            match next {
                Some((next_node_id, _)) => {
                    let next_id = *next_node_id;
                    let next_output = ctx.evaluate_pin(next_id, "image_out")?;
                    match next_output.into_image() {
                        Some(img) => {
                            current_image = img;
                            current_node_id = next_id;
                        }
                        None => break,
                    }
                }
                None => break,
            }
        }

        Ok(Some(current_image))
    }
}
