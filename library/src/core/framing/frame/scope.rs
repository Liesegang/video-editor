use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct EvaluationScope {
    pub(crate) time: f64,
    pub(crate) fps: f64,
    pub(crate) duration: f64,
    pub(crate) width: u64,
    pub(crate) height: u64,
}

impl EvaluationScope {
    pub(super) fn value(self, port: &str) -> Option<PropertyValue> {
        match port {
            TIME_PORT => Some(PropertyValue::Number(OrderedFloat(self.time))),
            FRAME_PORT => Some(PropertyValue::Integer(frame_at_time(self.time, self.fps))),
            FPS_PORT => Some(PropertyValue::Number(OrderedFloat(self.fps))),
            DURATION_PORT => Some(PropertyValue::Number(OrderedFloat(self.duration))),
            RESOLUTION_PORT => Some(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(self.width as f64),
                y: OrderedFloat(self.height as f64),
            })),
            _ => None,
        }
    }

    pub(super) fn as_inputs(self) -> HashMap<String, EvalOutput<PropertyValue>> {
        [
            TIME_PORT,
            FRAME_PORT,
            FPS_PORT,
            DURATION_PORT,
            RESOLUTION_PORT,
        ]
        .into_iter()
        .filter_map(|port| {
            self.value(port)
                .map(|value| (port.to_string(), EvalOutput::Produced(value)))
        })
        .collect()
    }
}

impl FrameEvaluator<'_> {
    pub(super) fn scope_for_node(
        &self,
        node_id: Uuid,
        global_time: f64,
    ) -> EvalResult<EvaluationScope> {
        self.scope_for_owner(PortOwner::Node(node_id), global_time, &mut HashSet::new())
    }

    /// Shared graph-time boundary used by frame and audio evaluation. The
    /// caller supplies the timeline time of the current Composition context
    /// and reusable recursion storage; Clip-local timing and explicit metadata
    /// wires are resolved without mutating any authored Project state.
    pub(crate) fn evaluate_owner_scope_with_scratch(
        &self,
        owner: PortOwner,
        timeline_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<EvaluationScope> {
        path.clear();
        self.scope_for_owner(owner, timeline_time, path)
    }

    pub(super) fn scope_for_owner(
        &self,
        owner: PortOwner,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<EvaluationScope> {
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = (|| {
            let mut scope = match owner {
                PortOwner::Composition(id) => {
                    let composition = self
                        .project
                        .get_composition(id)
                        .ok_or_else(|| missing_error(owner))?;
                    // Composition activity uses its authored half-open
                    // timeline range before any explicit Time input remaps.
                    if !global_time.is_finite()
                        || global_time < 0.0
                        || global_time >= composition.duration
                    {
                        return Ok(EvalOutput::NoOutput);
                    }
                    EvaluationScope {
                        time: global_time,
                        fps: composition.fps,
                        duration: composition.duration,
                        width: composition.width,
                        height: composition.height,
                    }
                }
                PortOwner::Track(id) => {
                    let composition_id = self
                        .project
                        .find_composition_for_track(id)
                        .ok_or_else(|| missing_error(owner))?;
                    match self.scope_for_owner(
                        PortOwner::Composition(composition_id),
                        global_time,
                        path,
                    )? {
                        EvalOutput::Produced(scope) => scope,
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    }
                }
                PortOwner::Clip(id) => {
                    let clip = self
                        .project
                        .get_clip(id)
                        .ok_or_else(|| missing_error(owner))?;
                    let track_id = self
                        .project
                        .find_track_for_clip(id)
                        .ok_or_else(|| missing_error(owner))?;
                    let mut inherited = match self.scope_for_owner(
                        PortOwner::Track(track_id),
                        global_time,
                        path,
                    )? {
                        EvalOutput::Produced(scope) => scope,
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    };
                    // Clip activity is start-inclusive and end-exclusive.
                    if inherited.time < clip.start_time.into_inner()
                        || inherited.time >= clip.end_time()
                    {
                        return Ok(EvalOutput::NoOutput);
                    }
                    inherited.duration = clip.duration.into_inner();
                    match self.apply_metadata_inputs(owner, global_time, path, &mut inherited)? {
                        EvalOutput::Produced(()) => {}
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    }
                    inherited.time = clip.local_time(inherited.time);
                    return Ok(EvalOutput::Produced(inherited));
                }
                PortOwner::Node(id) => {
                    let container = self
                        .project
                        .find_node_container(id)
                        .ok_or_else(|| missing_error(owner))?;
                    let container_owner = match container {
                        NodeContainer::Composition(id) => PortOwner::Composition(id),
                        NodeContainer::Track(id) => PortOwner::Track(id),
                        NodeContainer::Clip(id) => PortOwner::Clip(id),
                    };
                    match self.scope_for_owner(container_owner, global_time, path)? {
                        EvalOutput::Produced(scope) => scope,
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    }
                }
            };
            match self.apply_metadata_inputs(owner, global_time, path, &mut scope)? {
                EvalOutput::Produced(()) => {}
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            }
            Ok(EvalOutput::Produced(scope))
        })();
        path.remove(&owner);
        result
    }

    fn apply_metadata_inputs(
        &self,
        owner: PortOwner,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
        scope: &mut EvaluationScope,
    ) -> EvalResult<()> {
        for port in [DURATION_PORT, RESOLUTION_PORT, TIME_PORT] {
            let target = PortAddress::new(owner, port);
            let connection = match self.single_connection_to(&target)? {
                EvalOutput::Produced(connection) => connection,
                EvalOutput::NoOutput => continue,
            };
            let value = match self.resolve_metadata_value(&connection.from, global_time, path)? {
                EvalOutput::Produced(value) => value,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
            match (port, value) {
                (TIME_PORT, value) => scope.time = required_number(value, port)?,
                (DURATION_PORT, value) => scope.duration = required_number(value, port)?,
                (RESOLUTION_PORT, PropertyValue::Vec2(value)) => {
                    scope.width = value.x.into_inner().max(1.0) as u64;
                    scope.height = value.y.into_inner().max(1.0) as u64;
                }
                _ => return Err(invalid_value(port)),
            }
        }
        Ok(EvalOutput::Produced(()))
    }
}
