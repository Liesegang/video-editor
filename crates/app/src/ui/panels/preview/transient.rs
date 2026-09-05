//! Immutable gesture projection, shared by every editing surface before the
//! ordinary Preview request compiles the matching RenderPlan.

use std::collections::HashMap;
use std::sync::Arc;

use library::model::authoring::{AuthoringProject, ProjectRevision};

use crate::state::authoring::{AuthoringUiState, TransientPropertyEdit};
use crate::ui::automation_lanes;

use super::{gizmo, text_editor, AuthoringPreviewRuntime};

struct TransientProjectProjection {
    revision: ProjectRevision,
    upstream_edit: Option<u64>,
    edit: u64,
    project: Arc<AuthoringProject>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum TransientProjectionStage {
    Text,
    Property,
    Transform,
}

type ProjectedEdit = Result<(Arc<AuthoringProject>, Option<u64>), String>;

#[derive(Default)]
pub(super) struct TransientProjectionCache {
    entries: HashMap<TransientProjectionStage, TransientProjectProjection>,
}

impl TransientProjectionCache {
    pub(super) fn project(
        &mut self,
        stage: TransientProjectionStage,
        revision: ProjectRevision,
        upstream_edit: Option<u64>,
        edit: Option<u64>,
        source: &Arc<AuthoringProject>,
        apply: impl FnOnce(&Arc<AuthoringProject>) -> ProjectedEdit,
    ) -> ProjectedEdit {
        let Some(edit) = edit else {
            self.entries.remove(&stage);
            return Ok((Arc::clone(source), None));
        };
        if let Some(cached) = self.entries.get(&stage).filter(|cached| {
            cached.revision == revision
                && cached.upstream_edit == upstream_edit
                && cached.edit == edit
        }) {
            return Ok((Arc::clone(&cached.project), Some(edit)));
        }

        // An invalid held edit must not reuse or publish the preceding edit.
        self.entries.remove(&stage);
        let (projected, applied_edit) = apply(source)?;
        if applied_edit == Some(edit) {
            self.entries.insert(
                stage,
                TransientProjectProjection {
                    revision,
                    upstream_edit,
                    edit,
                    project: Arc::clone(&projected),
                },
            );
        }
        Ok((projected, applied_edit))
    }
}

impl AuthoringPreviewRuntime {
    pub(super) fn project_for_preview(
        &mut self,
        project: &Arc<AuthoringProject>,
        revision: ProjectRevision,
        state: &AuthoringUiState,
    ) -> ProjectedEdit {
        let text_digest = text_editor::transient_edit_digest(state);
        let (projected, text_edit) = self.transient_projections.project(
            TransientProjectionStage::Text,
            revision,
            None,
            text_digest,
            project,
            |source| Ok(text_editor::transient_render_project(source, state)),
        )?;
        let property = property_edit(revision, state)?;
        let property_digest = property.as_ref().map(TransientPropertyEdit::digest);
        let (projected, property_edit) = self.transient_projections.project(
            TransientProjectionStage::Property,
            revision,
            text_edit,
            property_digest,
            &projected,
            |source| match property {
                Some(edit) => edit
                    .project(source)
                    .map(|project| (Arc::new(project), property_digest))
                    .map_err(|error| format!("Preview property: {error}")),
                None => Ok((Arc::clone(source), None)),
            },
        )?;
        let upstream_edit = combine_transient_edits(text_edit, property_edit);
        let transform_digest = gizmo::transient_edit_digest(state);
        let (projected, transform_edit) = self.transient_projections.project(
            TransientProjectionStage::Transform,
            revision,
            upstream_edit,
            transform_digest,
            &projected,
            |source| Ok(gizmo::transient_render_project(source, state)),
        )?;
        Ok((
            projected,
            combine_transient_edits(upstream_edit, transform_edit),
        ))
    }
}

fn property_edit(
    revision: ProjectRevision,
    state: &AuthoringUiState,
) -> Result<Option<TransientPropertyEdit>, String> {
    // Curve already owns the draft. Derive its property edit on demand, without
    // a second copy of gesture state or an upsert that would replace key IDs.
    if let Some(drag) = state.curve_editor.drag.as_ref().filter(|drag| {
        drag.source_revision == revision
            && (drag.projected_time != drag.original_time
                || drag.projected_value != drag.original_value)
    }) {
        let target =
            automation_lanes::keyframe_target(&drag.lane).map_err(|error| error.to_string())?;
        return Ok(Some(TransientPropertyEdit::keyframe(
            revision,
            target,
            drag.keyframe_id,
            drag.projected_time,
            drag.projected_value.clone(),
        )));
    }
    Ok(state
        .inspector
        .transient_property_edit
        .as_ref()
        .filter(|edit| edit.source_revision == revision)
        .cloned())
}

fn combine_transient_edits(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    match (first, second) {
        (None, None) => None,
        (Some(value), None) | (None, Some(value)) => Some(value),
        (Some(first), Some(second)) => Some(first.rotate_left(17) ^ second.rotate_right(11)),
    }
}
