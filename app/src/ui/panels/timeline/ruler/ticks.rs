pub(super) fn get_nice_time_intervals(
    pixels_per_unit: f32,
    nice_steps: &[f32],
    target_pixels_per_major_tick: f32,
) -> (f32, f32) {
    let mut major_interval = *nice_steps.last().unwrap_or(&1.0); // Default to largest if none fit

    for &step in nice_steps.iter() {
        let pixels_for_this_step = step * pixels_per_unit;
        if pixels_for_this_step > target_pixels_per_major_tick {
            major_interval = step;
            break;
        }
    }

    // If still too small (zoom out extreme), keep doubling until it fits
    while major_interval * pixels_per_unit < target_pixels_per_major_tick {
        major_interval *= 2.0;
    }

    let minor_interval = if major_interval >= 10.0 {
        major_interval / 5.0
    } else if major_interval >= 5.0 {
        major_interval / 5.0
    } else if major_interval >= 2.0 {
        major_interval / 5.0
    } else {
        // major_interval is 1.0 or smaller
        major_interval / 5.0 // This will give 0.5s or 0.5 frames
    };

    (major_interval, minor_interval)
}

pub(super) fn get_frame_intervals(pixels_per_unit: f32, _fps: f32) -> (f32, f32, bool) {
    // bool indicates if it's purely frames
    // pixels_per_unit is pixels per frame
    let nice_steps = &[1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0];
    let (major_interval_frames, minor_interval_frames) =
        get_nice_time_intervals(pixels_per_unit, nice_steps, 50.0);

    // Return frame intervals directly
    (
        major_interval_frames,
        minor_interval_frames,
        true, // This mode is purely frames
    )
}
