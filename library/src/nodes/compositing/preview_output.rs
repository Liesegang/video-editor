//! Evaluator for compositing.preview_output — terminal sink node.
//!
//! Pulls the upstream image from `image_in` and returns it as the
//! composition's final preview output.

use uuid::Uuid;

use crate::error::LibraryError;
use crate::pipeline::context::EvalContext;
use crate::pipeline::evaluator::NodeEvaluator;
use crate::pipeline::output::PinValue;

pub struct PreviewOutputEvaluator;

impl NodeEvaluator for PreviewOutputEvaluator {
    fn handles(&self) -> &[&str] {
        &["compositing.preview_output"]
    }

    fn evaluate(
        &self,
        node_id: Uuid,
        pin_name: &str,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        // The preview output node has no output pins, but the engine
        // calls evaluate with pin_name="image_in" to pull the final image.
        if pin_name == "image_in" {
            ctx.pull_input_value(node_id, "image_in")
        } else {
            Ok(PinValue::None)
        }
    }
}
