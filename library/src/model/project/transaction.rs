//! Shared primitives for validating and routing Project graph transactions.

use super::{NodeContainer, PortOwner, ProjectGraphError};

pub(super) fn port_owner_for_container(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

/// Return only a validation failure introduced by the candidate mutation.
/// Existing malformed authored state remains inspectable and recoverable.
pub(super) fn first_new_project_validation_error(
    baseline: &[ProjectGraphError],
    current: Vec<ProjectGraphError>,
) -> Option<ProjectGraphError> {
    let mut unmatched_baseline = baseline.to_vec();
    current.into_iter().find(|error| {
        let Some(index) = unmatched_baseline
            .iter()
            .position(|baseline_error| baseline_error == error)
        else {
            return true;
        };
        unmatched_baseline.remove(index);
        false
    })
}
