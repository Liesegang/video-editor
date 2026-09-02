//! Composition, Track, and Clip traversal for the Image graph.
//!
//! This module owns authored container ranges, output bindings, and renderer
//! group boundaries. Image-producing Nodes are delegated to `image_graph`;
//! container semantics do not inspect Node implementation details.

use std::collections::HashSet;

use ordered_float::OrderedFloat;
use uuid::Uuid;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error, transparent_background};
use crate::error::LibraryError;
use crate::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::project::{Composition, EvalOutput, EvalResult, PortOwner};
use crate::plugin::ResolvedNodeInputs;

impl FrameEvaluator<'_> {
    pub fn evaluate(
        &self,
        frame_number: u64,
        render_scale: f64,
        region: Option<Region>,
    ) -> Result<FrameInfo, LibraryError> {
        if let Some(error) = self.project.validate_connections().into_iter().next() {
            return Err(LibraryError::Validation(error.to_string()));
        }
        let global_time = frame_number as f64 / self.composition.fps;
        let mut frame = FrameInfo {
            width: self.composition.width,
            height: self.composition.height,
            // The root Composition is the only boundary that materializes a
            // normal NoOutput as its configured background/transparent canvas.
            background_color: self.composition.background_color.clone(),
            color_profile: self.composition.color_profile.clone(),
            render_scale: OrderedFloat(render_scale),
            now_time: OrderedFloat(global_time),
            region,
            items: Vec::new(),
        };
        frame.items = match self.collect_composition_items(
            self.composition,
            global_time,
            &mut HashSet::new(),
        )? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => Vec::new(),
        };
        Ok(frame)
    }

    pub(super) fn collect_composition_items(
        &self,
        composition: &Composition,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Vec<FrameItem>> {
        let owner = PortOwner::Composition(composition.id);
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
            EvalOutput::Produced(_) => {}
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        }
        let items = self.collect_container_image_items(owner, global_time, path);
        path.remove(&owner);
        items
    }

    pub(super) fn collect_track(
        &self,
        track_id: Uuid,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Track(track_id);
        let track = self
            .project
            .get_track(track_id)
            .ok_or_else(|| missing_error(owner))?;
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let scope = match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let items = match self.collect_container_image_items(owner, global_time, path)? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
        let item = FrameItem::Group(FrameGroup {
            source_id: track.id,
            kind: FrameGroupKind::Track,
            width: scope.width,
            height: scope.height,
            background_color: transparent_background(),
            inherited_transforms: Vec::new(),
            transform: self
                .context(composition, Some(&inputs))
                .build_transform(&track.properties, scope.time),
            blend_mode: track.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items,
        });
        path.remove(&owner);
        Ok(EvalOutput::Produced(item))
    }

    pub(super) fn collect_clip(
        &self,
        clip_id: Uuid,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Clip(clip_id);
        let clip = self
            .project
            .get_clip(clip_id)
            .ok_or_else(|| missing_error(owner))?;
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let scope = match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let items = match self.collect_container_image_items(owner, global_time, path)? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
        let item = FrameItem::Group(FrameGroup {
            source_id: clip.id,
            kind: FrameGroupKind::Clip,
            width: scope.width,
            height: scope.height,
            background_color: transparent_background(),
            inherited_transforms: Vec::new(),
            transform: self
                .context(composition, Some(&inputs))
                .build_transform(&clip.properties, scope.time),
            blend_mode: clip.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items,
        });
        path.remove(&owner);
        Ok(EvalOutput::Produced(item))
    }

    fn collect_container_image_items(
        &self,
        owner: PortOwner,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Vec<FrameItem>> {
        let mut candidates = Vec::new();
        for source in self.project.container_image_sources(owner) {
            // Every child, including an explicitly bound direct Node, goes
            // through its own authoritative owner scope. Passing the
            // container scope directly here used to bypass the Node's Time
            // input only for direct output bindings.
            let item = self.collect_owner_output(source.source, global_time, path)?;
            candidates.push(item);
        }
        Ok(aggregate_outputs(candidates))
    }
}

fn aggregate_outputs(items: Vec<EvalOutput<FrameItem>>) -> EvalOutput<Vec<FrameItem>> {
    let items = items
        .into_iter()
        .filter_map(|item| match item {
            EvalOutput::Produced(item) => Some(item),
            EvalOutput::NoOutput => None,
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        EvalOutput::NoOutput
    } else {
        EvalOutput::Produced(items)
    }
}
