use std::sync::Arc;

use egui_phosphor::regular as icons;
use library::model::authoring::{Attachment, AttachmentId, AttachmentOwner, AttachmentStage};
use library::model::project::connection::PortDataType;

#[derive(Clone, Debug)]
pub(super) struct EffectDragPayload {
    pub attachment_id: AttachmentId,
    pub owner: AttachmentOwner,
    pub source_stage: AttachmentStage,
    pub media_type: PortDataType,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EffectDropTarget {
    pub stage: AttachmentStage,
    pub index: usize,
}

pub(super) struct DropSlotResponse {
    pub hovered: bool,
    pub dropped: Option<Arc<EffectDragPayload>>,
}

const PREVIEW_TARGET_MEMORY: &str = "inspector.effect_stack.preview_target";

pub(super) fn active_payload(ctx: &egui::Context) -> Option<Arc<EffectDragPayload>> {
    egui::DragAndDrop::payload(ctx)
}

pub(super) fn preview_target(ctx: &egui::Context) -> Option<EffectDropTarget> {
    ctx.data(|data| data.get_temp(egui::Id::new(PREVIEW_TARGET_MEMORY)))
}

pub(super) fn store_preview_target(ctx: &egui::Context, target: Option<EffectDropTarget>) {
    ctx.data_mut(|data| {
        let id = egui::Id::new(PREVIEW_TARGET_MEMORY);
        if let Some(target) = target {
            data.insert_temp(id, target);
        } else {
            data.remove::<EffectDropTarget>(id);
        }
    });
}

pub(super) fn drag_handle(
    ui: &mut egui::Ui,
    attachment: &Attachment,
    media_type: PortDataType,
    title: &str,
) -> egui::Response {
    let response = ui
        .add_sized(
            [20.0, 20.0],
            egui::Label::new(egui::RichText::new(icons::DOTS_SIX_VERTICAL).weak())
                .sense(egui::Sense::drag()),
        )
        .on_hover_text("Drag to reorder or move this Effect to another stage")
        .on_hover_cursor(egui::CursorIcon::Grab);
    response.dnd_set_drag_payload(EffectDragPayload {
        attachment_id: attachment.id,
        owner: attachment.owner.clone(),
        source_stage: attachment.stage,
        media_type,
        title: title.to_string(),
    });
    let dragging =
        active_payload(ui.ctx()).is_some_and(|payload| payload.attachment_id == attachment.id);
    crate::qa::register_component_with_metadata(
        format!("inspector.effect_drag_handle:{}", attachment.id),
        "effect_drag_handle",
        response.rect,
        true,
        Some(serde_json::json!({
            "attachment_id": attachment.id,
            "source_stage": format!("{:?}", attachment.stage),
            "dragging": dragging,
        })),
    );
    response
}

pub(super) fn drop_slot(
    ui: &mut egui::Ui,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
    media_type: PortDataType,
    index: usize,
    destination_empty: bool,
    previous_target: Option<EffectDropTarget>,
) -> DropSlotResponse {
    let target = EffectDropTarget { stage, index };
    let active = active_payload(ui.ctx());
    let compatible = active
        .as_ref()
        .is_some_and(|payload| payload.owner == *owner && payload.media_type == media_type);
    let previewed = compatible && previous_target == Some(target);
    let height = if compatible {
        if destination_empty || previewed {
            30.0
        } else {
            10.0
        }
    } else {
        2.0
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(1.0), height),
        egui::Sense::hover(),
    );
    let hovered_payload = response.dnd_hover_payload::<EffectDragPayload>();
    let hovered = compatible
        && hovered_payload
            .as_ref()
            .is_some_and(|payload| payload.owner == *owner && payload.media_type == media_type);
    let dropped = if compatible {
        response.dnd_release_payload::<EffectDragPayload>()
    } else {
        None
    };

    if compatible {
        let accent = ui.visuals().selection.stroke.color;
        if previewed || hovered {
            let preview_rect = rect.shrink2(egui::vec2(2.0, 2.0));
            ui.painter().rect(
                preview_rect,
                4.0,
                ui.visuals().selection.bg_fill.gamma_multiply(0.55),
                egui::Stroke::new(1.5, accent),
                egui::StrokeKind::Inside,
            );
            if rect.height() >= 24.0 {
                let title = active
                    .as_ref()
                    .map_or("Move Effect here", |payload| payload.title.as_str());
                ui.painter().text(
                    rect.left_center() + egui::vec2(9.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::TextStyle::Small.resolve(ui.style()),
                    accent,
                );
            }
        } else {
            ui.painter().hline(
                rect.x_range().shrink(4.0),
                rect.center().y,
                egui::Stroke::new(1.0, accent.gamma_multiply(0.35)),
            );
        }
    }

    crate::qa::register_component_with_metadata(
        format!("inspector.effect_drop:{}:{index}", stage_id(stage)),
        "effect_drop_slot",
        rect,
        compatible,
        Some(serde_json::json!({
            "dragging": active.is_some(),
            "compatible": compatible,
            "hovered": hovered,
            "target_stage": format!("{stage:?}"),
            "target_index": index,
            "attachment_id": active.as_ref().map(|payload| payload.attachment_id),
            "source_stage": active.as_ref().map(|payload| format!("{:?}", payload.source_stage)),
        })),
    );
    DropSlotResponse { hovered, dropped }
}

pub(super) fn paint_drag_preview(ui: &egui::Ui, payload: &EffectDragPayload) {
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return;
    };
    egui::Area::new(egui::Id::new((
        "effect_drag_preview",
        payload.attachment_id,
    )))
    .order(egui::Order::Tooltip)
    .fixed_pos(pointer + egui::vec2(14.0, 14.0))
    .interactable(false)
    .show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icons::DOTS_SIX_VERTICAL).weak());
                ui.label(egui::RichText::new(&payload.title).strong());
            });
        });
    });
}

pub(super) const fn stage_id(stage: AttachmentStage) -> &'static str {
    match stage {
        AttachmentStage::ItemTimeMap => "item_time_map",
        AttachmentStage::ItemPreTransform => "item_pre_transform",
        AttachmentStage::ItemPostTransform => "item_post_transform",
        AttachmentStage::TrackPostComposite => "track_post_composite",
        AttachmentStage::TimelinePostComposite => "timeline_post_composite",
        AttachmentStage::AudioPreFader => "audio_pre_fader",
        AttachmentStage::AudioPostFader => "audio_post_fader",
        AttachmentStage::TrackPostMix => "track_post_mix",
        AttachmentStage::TimelinePostMix => "timeline_post_mix",
    }
}
