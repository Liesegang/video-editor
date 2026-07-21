use super::*;

impl FrameEvaluator<'_> {
    pub(super) fn collect_composition_instance(
        &self,
        node: &Node,
        instance: &crate::model::CompositionInstanceContent,
        scope: EvaluationScope,
        path: &mut HashSet<PortOwner>,
        inputs: &ResolvedNodeInputs,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Node(node.id);
        let target = self
            .project
            .get_composition(instance.composition_id)
            .ok_or_else(|| missing_error(PortOwner::Composition(instance.composition_id)))?;
        let mut item =
            match self.collect_owner_output(PortOwner::Composition(target.id), scope.time, path)? {
                EvalOutput::Produced(item) => item,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        let target_scope = match self.scope_for_owner(
            PortOwner::Composition(target.id),
            scope.time,
            &mut HashSet::new(),
        )? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        neutralize_root_blend(&mut item);
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let context = self.context(composition, Some(inputs));
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::CompositionInstance,
            width: target_scope.width,
            height: target_scope.height,
            background_color: transparent(),
            transform: context.build_transform(node.properties(), scope.time),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items: vec![item],
        })))
    }

    pub(super) fn composition_instance_target_scope(
        &self,
        node_id: Uuid,
        instance: &crate::model::CompositionInstanceContent,
        timeline_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<EvaluationScope> {
        let placement_scope =
            match self.scope_for_owner(PortOwner::Node(node_id), timeline_time, path)? {
                EvalOutput::Produced(scope) => scope,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        self.scope_for_owner(
            PortOwner::Composition(instance.composition_id),
            placement_scope.time,
            path,
        )
    }
}
