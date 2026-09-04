use library::editor::TimelineEditorService;
use library::model::authoring::{MediaTime, TimelineInterval, TimelineItem};

use crate::state::authoring::AuthoringUiState;
use crate::ui::widgets::property_drag_value::FloatDragValueConfig;

pub(super) fn timing_section(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
) {
    egui::CollapsingHeader::new("Timing")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new(("inspector.timing", item.id))
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Start");
                    let start =
                        ui.add(seconds_drag_config(0.0).widget(&mut state.inspector.start_seconds));
                    crate::qa::register_component(
                        format!("inspector.timing.start:{}", item.id),
                        "inspector_timing_control",
                        start.rect,
                    );
                    ui.end_row();

                    ui.label("Duration");
                    let duration = ui.add(
                        seconds_drag_config(1.0 / 1_000.0)
                            .widget(&mut state.inspector.duration_seconds),
                    );
                    crate::qa::register_component(
                        format!("inspector.timing.duration:{}", item.id),
                        "inspector_timing_control",
                        duration.rect,
                    );
                    ui.end_row();

                    commit_start(state, service, item, &start);
                    commit_duration(state, service, item, &duration);
                });
        });
}

fn commit_start(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    response: &egui::Response,
) {
    if !numeric_finished(response)
        || state.inspector.start_seconds == item.interval.start.to_seconds_f64()
    {
        return;
    }
    let Ok(new_start) =
        MediaTime::from_seconds_f64(state.inspector.start_seconds.max(0.0), 1_000_000)
    else {
        return;
    };
    if let Err(error) = service.move_item(item.id, item.track_id, new_start, item.layer) {
        state.error = Some(error.to_string());
    }
}

fn commit_duration(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    response: &egui::Response,
) {
    if !numeric_finished(response)
        || state.inspector.duration_seconds == item.interval.duration.to_seconds_f64()
    {
        return;
    }
    let (Ok(start), Ok(duration)) = (
        MediaTime::from_seconds_f64(state.inspector.start_seconds.max(0.0), 1_000_000),
        MediaTime::from_seconds_f64(
            state.inspector.duration_seconds.max(1.0 / 1_000.0),
            1_000_000,
        ),
    ) else {
        return;
    };
    let Ok(interval) = TimelineInterval::new(start, duration) else {
        return;
    };
    if let Err(error) = service.trim_item(item.id, interval) {
        state.error = Some(error.to_string());
    }
}

fn seconds_drag_config(minimum: f64) -> FloatDragValueConfig {
    FloatDragValueConfig {
        speed: 0.01,
        suffix: " s".to_string(),
        hard_min: Some(minimum),
        hard_max: None,
    }
}

fn numeric_finished(response: &egui::Response) -> bool {
    response.drag_stopped()
        || response.lost_focus()
        || (response.has_focus()
            && response
                .ctx
                .input(|input| input.key_pressed(egui::Key::Enter)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_controls_share_the_property_drag_configuration() {
        let start = seconds_drag_config(0.0);
        let duration = seconds_drag_config(0.001);
        assert_eq!(start.speed, 0.01);
        assert_eq!(start.suffix, " s");
        assert_eq!(start.hard_min, Some(0.0));
        assert_eq!(duration.hard_min, Some(0.001));
    }
}
