use crate::model::authoring::{CompositionInstance, DurationPolicy, TimelineInterval};

/// Convert an owning Timeline time to the Item's authored local time.
pub fn item_local_time(placement: TimelineInterval, timeline_time: f64) -> f64 {
    timeline_time - placement.start.into_inner()
}

/// Local time used by direct editors when the playhead is outside the Item.
pub fn editable_item_local_time(placement: TimelineInterval, timeline_time: f64) -> f64 {
    item_local_time(placement, timeline_time).clamp(0.0, placement.duration.into_inner())
}

pub fn map_composition_time(
    instance: &CompositionInstance,
    placement: TimelineInterval,
    definition_duration: f64,
    timeline_time: f64,
) -> Result<Option<f64>, String> {
    if !definition_duration.is_finite() || definition_duration < 0.0 {
        return Err("Nested Timeline duration must be finite and non-negative".to_string());
    }
    if !instance.time_map.source_start.is_finite() || !instance.time_map.playback_rate.is_finite() {
        return Err("Composition time map must be finite".to_string());
    }
    if !placement.contains(timeline_time) {
        return Ok(None);
    }
    let local = item_local_time(placement, timeline_time);
    let mapped = match &instance.duration_policy {
        DurationPolicy::Fixed => {
            if local >= definition_duration {
                return Ok(None);
            }
            local
        }
        DurationPolicy::Scale => {
            let placement_duration = placement.duration.into_inner();
            if placement_duration == 0.0 {
                return Ok(None);
            }
            local * definition_duration / placement_duration
        }
        DurationPolicy::Loop => {
            if definition_duration == 0.0 {
                return Ok(None);
            }
            local.rem_euclid(definition_duration)
        }
        DurationPolicy::Responsive {
            intro_end,
            outro_start,
        } => map_responsive_time(
            local,
            placement.duration.into_inner(),
            definition_duration,
            intro_end.into_inner(),
            outro_start.into_inner(),
        )?,
    };
    Ok(Some(
        instance.time_map.source_start.into_inner()
            + mapped * instance.time_map.playback_rate.into_inner(),
    ))
}

fn map_responsive_time(
    local: f64,
    placement_duration: f64,
    definition_duration: f64,
    intro_end: f64,
    outro_start: f64,
) -> Result<f64, String> {
    if !intro_end.is_finite()
        || !outro_start.is_finite()
        || intro_end < 0.0
        || intro_end > outro_start
        || outro_start > definition_duration
    {
        return Err("Responsive duration markers are invalid".to_string());
    }
    let outro_duration = definition_duration - outro_start;
    let minimum_duration = intro_end + outro_duration;
    if placement_duration < minimum_duration {
        return Err(format!(
            "Responsive placement must be at least {minimum_duration} seconds"
        ));
    }
    if local < intro_end {
        return Ok(local);
    }
    let target_outro_start = placement_duration - outro_duration;
    if local >= target_outro_start {
        return Ok(outro_start + local - target_outro_start);
    }
    let target_middle = target_outro_start - intro_end;
    let source_middle = outro_start - intro_end;
    if target_middle == 0.0 || source_middle == 0.0 {
        Ok(intro_end)
    } else {
        Ok(intro_end + (local - intro_end) * source_middle / target_middle)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ordered_float::OrderedFloat;

    use super::*;
    use crate::model::authoring::{TimeMap, TimelineId};

    fn instance(policy: DurationPolicy) -> CompositionInstance {
        CompositionInstance {
            timeline_id: TimelineId::new(),
            time_map: TimeMap::default(),
            duration_policy: policy,
            parameter_overrides: HashMap::new(),
        }
    }

    #[test]
    fn moving_outer_placement_preserves_nested_local_time() {
        let nested = instance(DurationPolicy::Fixed);
        let first = map_composition_time(
            &nested,
            TimelineInterval::new(2.0, 5.0).expect("interval"),
            5.0,
            3.25,
        )
        .expect("mapping");
        let moved = map_composition_time(
            &nested,
            TimelineInterval::new(20.0, 5.0).expect("interval"),
            5.0,
            21.25,
        )
        .expect("mapping");
        assert_eq!(first, Some(1.25));
        assert_eq!(moved, first);
    }

    #[test]
    fn item_authoring_time_is_local_and_editor_time_is_bounded() {
        let placement = TimelineInterval::new(10.0, 4.0).expect("interval");
        assert_eq!(item_local_time(placement, 11.5), 1.5);
        assert_eq!(editable_item_local_time(placement, 8.0), 0.0);
        assert_eq!(editable_item_local_time(placement, 20.0), 4.0);
    }

    #[test]
    fn scale_loop_and_responsive_have_distinct_time_semantics() {
        let placement = TimelineInterval::new(0.0, 8.0).expect("interval");
        assert_eq!(
            map_composition_time(&instance(DurationPolicy::Scale), placement, 4.0, 6.0)
                .expect("scale"),
            Some(3.0)
        );
        assert_eq!(
            map_composition_time(&instance(DurationPolicy::Loop), placement, 3.0, 7.0)
                .expect("loop"),
            Some(1.0)
        );
        let responsive = instance(DurationPolicy::Responsive {
            intro_end: OrderedFloat(1.0),
            outro_start: OrderedFloat(3.0),
        });
        assert_eq!(
            map_composition_time(&responsive, placement, 4.0, 7.5).expect("responsive"),
            Some(3.5)
        );
    }
}
