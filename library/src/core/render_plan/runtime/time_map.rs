//! Exact local-time mapping for nested Timeline instances.

use super::*;

pub(crate) fn map_composition_time(
    item: &TimelineItem,
    definition_duration: MediaTime,
    policy: &DurationPolicy,
    timeline_time: MediaTime,
) -> Result<Option<MediaTime>, String> {
    if !item.interval.contains(timeline_time)? {
        return Ok(None);
    }
    let local = timeline_time.checked_sub(item.interval.start)?;
    let mapped = match policy {
        DurationPolicy::Fixed => {
            if local >= definition_duration {
                return Ok(None);
            }
            local
        }
        DurationPolicy::Scale => {
            if item.interval.duration == MediaTime::zero() {
                return Ok(None);
            }
            scale_media_time(local, definition_duration, item.interval.duration)?
        }
        DurationPolicy::Loop => {
            if definition_duration == MediaTime::zero() {
                return Ok(None);
            }
            media_time_remainder(local, definition_duration)?
        }
        DurationPolicy::Responsive {
            intro_end,
            outro_start,
        } => responsive_time(
            local,
            item.interval.duration,
            definition_duration,
            *intro_end,
            *outro_start,
        )?,
    };
    mapped
        .checked_mul_rate(item.time_map.playback_rate)?
        .checked_add(item.time_map.source_start)
        .map(Some)
}

/// Chooses the unique placement time for Fixed/Scale/Responsive mappings and
/// the first valid loop occurrence for Loop. This is used only when Preview
/// enters a concrete nested instance and its playhead is expressed in local
/// Timeline time.
pub(super) fn unmap_composition_time(
    item: &TimelineItem,
    definition_duration: MediaTime,
    policy: &DurationPolicy,
    definition_time: MediaTime,
) -> Result<MediaTime, String> {
    let mapped = definition_time
        .checked_sub(item.time_map.source_start)?
        .checked_div_rate(item.time_map.playback_rate)?;
    if mapped.is_negative() {
        return Err("Nested local time precedes the instance source start".to_string());
    }
    let local = match policy {
        DurationPolicy::Fixed => mapped,
        DurationPolicy::Scale => {
            if definition_duration == MediaTime::zero() {
                return Err("Cannot invert a zero-duration scaled Composition".to_string());
            }
            scale_media_time(mapped, item.interval.duration, definition_duration)?
        }
        DurationPolicy::Loop => {
            if definition_duration == MediaTime::zero() || mapped >= definition_duration {
                return Err("Loop preview time is outside the Composition definition".to_string());
            }
            mapped
        }
        DurationPolicy::Responsive {
            intro_end,
            outro_start,
        } => unmap_responsive_time(
            mapped,
            item.interval.duration,
            definition_duration,
            *intro_end,
            *outro_start,
        )?,
    };
    if local >= item.interval.duration {
        return Err("Nested local time is outside this Composition placement".to_string());
    }
    item.interval.start.checked_add(local)
}

fn unmap_responsive_time(
    mapped: MediaTime,
    placement_duration: MediaTime,
    definition_duration: MediaTime,
    intro_end: MediaTime,
    outro_start: MediaTime,
) -> Result<MediaTime, String> {
    if intro_end.is_negative() || intro_end > outro_start || outro_start > definition_duration {
        return Err("Responsive duration markers are invalid".to_string());
    }
    let outro_duration = definition_duration.checked_sub(outro_start)?;
    let minimum_duration = intro_end.checked_add(outro_duration)?;
    if placement_duration < minimum_duration {
        return Err("Responsive placement is shorter than its fixed intro/outro".to_string());
    }
    if mapped < intro_end {
        return Ok(mapped);
    }
    let target_outro_start = placement_duration.checked_sub(outro_duration)?;
    if mapped >= outro_start {
        return target_outro_start.checked_add(mapped.checked_sub(outro_start)?);
    }
    let source_middle = outro_start.checked_sub(intro_end)?;
    let target_middle = target_outro_start.checked_sub(intro_end)?;
    if source_middle == MediaTime::zero() || target_middle == MediaTime::zero() {
        return Ok(intro_end);
    }
    intro_end.checked_add(scale_media_time(
        mapped.checked_sub(intro_end)?,
        target_middle,
        source_middle,
    )?)
}

pub(super) fn responsive_time(
    local: MediaTime,
    placement_duration: MediaTime,
    definition_duration: MediaTime,
    intro_end: MediaTime,
    outro_start: MediaTime,
) -> Result<MediaTime, String> {
    if intro_end.is_negative() || intro_end > outro_start || outro_start > definition_duration {
        return Err("Responsive duration markers are invalid".to_string());
    }
    let outro_duration = definition_duration.checked_sub(outro_start)?;
    let minimum_duration = intro_end.checked_add(outro_duration)?;
    if placement_duration < minimum_duration {
        return Err("Responsive placement is shorter than its fixed intro/outro".to_string());
    }
    if local < intro_end {
        return Ok(local);
    }
    let target_outro_start = placement_duration.checked_sub(outro_duration)?;
    if local >= target_outro_start {
        return outro_start.checked_add(local.checked_sub(target_outro_start)?);
    }
    let target_middle = target_outro_start.checked_sub(intro_end)?;
    let source_middle = outro_start.checked_sub(intro_end)?;
    if target_middle == MediaTime::zero() || source_middle == MediaTime::zero() {
        return Ok(intro_end);
    }
    intro_end.checked_add(scale_media_time(
        local.checked_sub(intro_end)?,
        source_middle,
        target_middle,
    )?)
}

pub(super) fn scale_media_time(
    value: MediaTime,
    numerator: MediaTime,
    denominator: MediaTime,
) -> Result<MediaTime, String> {
    if denominator == MediaTime::zero() {
        return Err("Cannot scale media time by a zero duration".to_string());
    }
    let scaled_numerator = i128::from(value.value())
        .checked_mul(i128::from(numerator.value()))
        .and_then(|value| value.checked_mul(i128::from(denominator.timescale())))
        .ok_or_else(|| "Media time scaling overflowed".to_string())?;
    let scaled_denominator = i128::from(value.timescale())
        .checked_mul(i128::from(numerator.timescale()))
        .and_then(|value| value.checked_mul(i128::from(denominator.value())))
        .ok_or_else(|| "Media time scaling overflowed".to_string())?;
    media_time_from_ratio(scaled_numerator, scaled_denominator)
}

pub(super) fn media_time_remainder(
    value: MediaTime,
    modulus: MediaTime,
) -> Result<MediaTime, String> {
    if modulus <= MediaTime::zero() {
        return Err("Media time modulus must be positive".to_string());
    }
    let value_numerator = i128::from(value.value())
        .checked_mul(i128::from(modulus.timescale()))
        .ok_or_else(|| "Media time remainder overflowed".to_string())?;
    let modulus_numerator = i128::from(modulus.value())
        .checked_mul(i128::from(value.timescale()))
        .ok_or_else(|| "Media time remainder overflowed".to_string())?;
    let denominator = i128::from(value.timescale())
        .checked_mul(i128::from(modulus.timescale()))
        .ok_or_else(|| "Media time remainder overflowed".to_string())?;
    media_time_from_ratio(value_numerator.rem_euclid(modulus_numerator), denominator)
}

pub(super) fn media_time_from_ratio(
    numerator: i128,
    denominator: i128,
) -> Result<MediaTime, String> {
    if denominator <= 0 {
        return Err("Media time denominator must be positive".to_string());
    }
    if numerator == 0 {
        return Ok(MediaTime::zero());
    }
    let divisor = gcd(numerator.unsigned_abs(), denominator as u128);
    let numerator = i64::try_from(numerator / divisor as i128)
        .map_err(|_| "Media time numerator exceeds i64".to_string())?;
    let denominator = u32::try_from(denominator / divisor as i128)
        .map_err(|_| "Media time timescale exceeds u32".to_string())?;
    MediaTime::new(numerator, denominator)
}

pub(super) fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}
