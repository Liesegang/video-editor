//! One waveform request and painter shared by authoring surfaces.

use std::time::Duration;

use egui::{Color32, Painter, Rect, Stroke};
use library::audio::waveform::AudioWaveformWindow;
use library::editor::AuthoringWaveformService;

pub(crate) struct WaveformPaintRequest<'a> {
    pub service: &'a AuthoringWaveformService,
    pub context: &'a egui::Context,
    pub painter: &'a Painter,
    pub rect: Rect,
    pub clip_rect: Rect,
    pub path: &'a str,
    pub stream_index: Option<usize>,
    pub duration_seconds: Option<f64>,
    pub source_time_at_left: f64,
    pub source_time_at_right: f64,
    pub color: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WaveformPaintReport {
    pub segment_count: usize,
    pub requested_chunks: usize,
    pub ready_chunks: usize,
    pub failed_chunks: usize,
    pub settled: bool,
    pub ready: bool,
    pub complete: bool,
    pub truncated: bool,
    pub identity_ready: bool,
    pub no_output: bool,
    pub source_time_start: f64,
    pub source_time_end: f64,
    pub first_source_frame: u64,
    pub end_source_frame_exclusive: u64,
}

impl WaveformPaintReport {
    pub(crate) fn status(self) -> &'static str {
        if self.no_output {
            "no_output"
        } else if !self.identity_ready {
            "identity_failure"
        } else if !self.settled {
            "pending"
        } else if self.ready {
            "ready"
        } else if self.truncated {
            "truncated"
        } else {
            "failed"
        }
    }
}

pub(crate) fn paint_authoring_waveform(request: WaveformPaintRequest<'_>) -> WaveformPaintReport {
    let sample_rate = request.service.format().sample_rate;
    let visible = request.rect.intersect(request.clip_rect);
    let source_at = |x: f32| {
        let ratio = if request.clip_rect.width() > 0.0 {
            f64::from((x - request.clip_rect.left()) / request.clip_rect.width())
        } else {
            0.0
        };
        request.source_time_at_left
            + (request.source_time_at_right - request.source_time_at_left) * ratio
    };
    let source_time_start = source_at(visible.left());
    let source_time_end = source_at(visible.right());
    let Some((first_source_frame, mut end_source_frame_exclusive)) =
        interpolation_frame_bounds(source_time_start, source_time_end, sample_rate)
    else {
        return empty_report(source_time_start, source_time_end);
    };
    if let Some(duration) = request
        .duration_seconds
        .filter(|duration| duration.is_finite() && *duration > 0.0)
    {
        let media_end = (duration * f64::from(sample_rate)).ceil() as u64;
        end_source_frame_exclusive = end_source_frame_exclusive.min(media_end);
    }
    if !visible.is_positive() || first_source_frame >= end_source_frame_exclusive {
        return empty_report(source_time_start, source_time_end);
    }

    let window = request.service.request_window(
        request.path,
        request.stream_index,
        first_source_frame,
        end_source_frame_exclusive.saturating_sub(1),
    );
    let mut segment_count = 0;
    if let Some(window) = window.as_ref() {
        let source_frames_per_pixel = f64::from(sample_rate)
            * (request.source_time_at_right - request.source_time_at_left).abs()
            / f64::from(request.clip_rect.width().max(1.0));
        let step_width = if source_frames_per_pixel > 1_000.0 {
            2.0
        } else {
            1.0
        };
        let center_y = request.clip_rect.center().y;
        let maximum_height = request.clip_rect.height() * 0.42;
        let mut x = visible.left();
        while x < visible.right() {
            let end_x = (x + step_width).min(visible.right());
            if let Some((first, end)) =
                interpolation_frame_bounds(source_at(x), source_at(end_x), sample_rate)
            {
                let end = request
                    .duration_seconds
                    .filter(|duration| duration.is_finite() && *duration > 0.0)
                    .map_or(end, |duration| {
                        end.min((duration * f64::from(sample_rate)).ceil() as u64)
                    });
                if let Some(peak) = window.peak_between(first, end) {
                    let height = (peak.clamp(0.0, 1.0) * maximum_height).max(1.0);
                    request.painter.line_segment(
                        [
                            egui::pos2(x, center_y - height),
                            egui::pos2(x, center_y + height),
                        ],
                        Stroke::new(step_width.min(1.5), request.color),
                    );
                    segment_count += 1;
                }
            }
            x += step_width;
        }
    }

    let report = window_report(
        window.as_ref(),
        segment_count,
        source_time_start,
        source_time_end,
        first_source_frame,
        end_source_frame_exclusive,
    );
    if !report.settled {
        request
            .context
            .request_repaint_after(Duration::from_millis(16));
    }
    report
}

fn interpolation_frame_bounds(
    first_source_time: f64,
    second_source_time: f64,
    sample_rate: u32,
) -> Option<(u64, u64)> {
    if !first_source_time.is_finite() || !second_source_time.is_finite() || sample_rate == 0 {
        return None;
    }
    let minimum_time = first_source_time.min(second_source_time);
    let maximum_time = first_source_time.max(second_source_time);
    if maximum_time < 0.0 {
        return None;
    }
    let rate = f64::from(sample_rate);
    let first = (minimum_time.max(0.0) * rate).floor() as u64;
    let last_interpolation_frame = (maximum_time.max(0.0) * rate).floor() as u64;
    Some((first, last_interpolation_frame.saturating_add(2)))
}

fn empty_report(source_time_start: f64, source_time_end: f64) -> WaveformPaintReport {
    WaveformPaintReport {
        segment_count: 0,
        requested_chunks: 0,
        ready_chunks: 0,
        failed_chunks: 0,
        settled: true,
        ready: false,
        complete: true,
        truncated: false,
        identity_ready: true,
        no_output: true,
        source_time_start,
        source_time_end,
        first_source_frame: 0,
        end_source_frame_exclusive: 0,
    }
}

fn window_report(
    window: Option<&AudioWaveformWindow>,
    segment_count: usize,
    source_time_start: f64,
    source_time_end: f64,
    first_source_frame: u64,
    end_source_frame_exclusive: u64,
) -> WaveformPaintReport {
    WaveformPaintReport {
        segment_count,
        requested_chunks: window.map_or(0, AudioWaveformWindow::requested_chunks),
        ready_chunks: window.map_or(0, AudioWaveformWindow::ready_chunks),
        failed_chunks: window.map_or(0, AudioWaveformWindow::failed_chunks),
        settled: window.is_none_or(AudioWaveformWindow::is_settled),
        ready: window.is_some_and(AudioWaveformWindow::is_ready),
        complete: window.is_none_or(AudioWaveformWindow::is_complete),
        truncated: window.is_some_and(AudioWaveformWindow::is_truncated),
        identity_ready: window.is_some(),
        no_output: false,
        source_time_start,
        source_time_end,
        first_source_frame,
        end_source_frame_exclusive,
    }
}

#[cfg(test)]
mod tests {
    use super::interpolation_frame_bounds;

    #[test]
    fn forward_and_reverse_ranges_request_the_same_source_frames() {
        assert_eq!(interpolation_frame_bounds(1.25, 2.5, 100), Some((125, 252)));
        assert_eq!(interpolation_frame_bounds(2.5, 1.25, 100), Some((125, 252)));
    }

    #[test]
    fn negative_only_and_invalid_ranges_produce_no_decode_request() {
        assert_eq!(interpolation_frame_bounds(-2.0, -1.0, 48_000), None);
        assert_eq!(interpolation_frame_bounds(f64::NAN, 1.0, 48_000), None);
        assert_eq!(interpolation_frame_bounds(0.0, 1.0, 0), None);
    }
}
