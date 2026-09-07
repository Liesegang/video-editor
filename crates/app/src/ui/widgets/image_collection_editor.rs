//! One ordered Image Asset collection editor shared by Inspector and Nodes.

use std::sync::Arc;

use egui::{Popup, PopupCloseBehavior, Response, Sense, Stroke, StrokeKind};
use egui_phosphor::regular as icons;
use library::model::asset::{Asset, AssetKind};
use library::model::authoring::AuthoringProject;
use library::model::property::{ImageCollectionValue, IMAGE_COLLECTION_MAX_ASSETS};

use crate::state::authoring::AuthoringLibraryDrag;
use crate::ui::media_preview::{
    paint_media_preview_texture, preview_request_size, representative_source_time,
    AuthoringMediaPreviewService,
};

const COLLECTION_SCROLL_SOURCE: egui::containers::scroll_area::ScrollSource =
    egui::containers::scroll_area::ScrollSource {
        scroll_bar: true,
        drag: false,
        mouse_wheel: true,
    };

pub(crate) struct ImageCollectionEditorContext<'a> {
    pub(crate) project: &'a Arc<AuthoringProject>,
    pub(crate) media_previews: &'a mut AuthoringMediaPreviewService,
    pub(crate) library_drag: &'a mut Option<AuthoringLibraryDrag>,
}

pub(crate) struct ImageCollectionEdit {
    pub(crate) response: Response,
    pub(crate) changed: bool,
}

enum CollectionAction {
    Add(uuid::Uuid),
    Remove(usize),
    MoveUp(usize),
    MoveDown(usize),
}

pub(crate) fn image_collection_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    qa_id: &str,
    value: &mut ImageCollectionValue,
    context: ImageCollectionEditorContext<'_>,
) -> ImageCollectionEdit {
    let ImageCollectionEditorContext {
        project,
        media_previews,
        library_drag,
    } = context;
    let candidates = image_candidates(project);
    let first = value
        .assets
        .first()
        .and_then(|asset_id| candidates.iter().find(|asset| asset.id == *asset_id))
        .copied();
    let summary = if value.assets.is_empty() {
        "Analytic disc".to_string()
    } else if value.assets.len() == 1 {
        first.map_or_else(|| "1 missing image".to_string(), |asset| asset.name.clone())
    } else {
        format!("{} images", value.assets.len())
    };
    let mut action = None;
    let row = ui.horizontal(|ui| {
        if let Some(asset) = first {
            asset_thumbnail(ui, qa_id, "summary", project, media_previews, asset, 22.0);
        } else {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                icons::CIRCLE,
                egui::FontId::proportional(16.0),
                ui.visuals().weak_text_color(),
            );
        }
        ui.add_sized([154.0, 22.0], egui::Button::new(&summary).truncate())
            .on_hover_text(summary)
    });
    let selector = row.inner;
    Popup::menu(&selector)
        .id(id.with("image_collection_popup"))
        .width(340.0)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            ui.set_min_width(340.0);
            ui.strong("Sprite images");
            ui.weak("Ordered Image Assets; empty uses the analytic disc.");
            egui::ScrollArea::vertical()
                .id_salt(id.with("image_collection_scroll"))
                .max_height(360.0)
                .auto_shrink([false, true])
                .scroll_source(COLLECTION_SCROLL_SOURCE)
                .show(ui, |ui| {
                    if value.assets.is_empty() {
                        ui.weak("No images selected");
                    }
                    for (index, asset_id) in value.assets.iter().copied().enumerate() {
                        let asset = candidates
                            .iter()
                            .find(|asset| asset.id == asset_id)
                            .copied();
                        selected_asset_row(
                            ui,
                            qa_id,
                            project,
                            media_previews,
                            index,
                            asset_id,
                            asset,
                            value.assets.len(),
                            &mut action,
                        );
                    }
                    ui.separator();
                    ui.strong("Add from Project Assets");
                    let full = value.assets.len() >= IMAGE_COLLECTION_MAX_ASSETS;
                    for asset in candidates.iter().copied() {
                        if value.assets.contains(&asset.id) {
                            continue;
                        }
                        let candidate = ui
                            .horizontal(|ui| {
                                asset_thumbnail(
                                    ui,
                                    qa_id,
                                    &format!("candidate:{}", asset.id),
                                    project,
                                    media_previews,
                                    asset,
                                    32.0,
                                );
                                ui.add_enabled_ui(!full, |ui| {
                                    ui.add_sized(
                                        [270.0, 32.0],
                                        egui::Button::new(format!(
                                            "{} {}",
                                            icons::PLUS,
                                            asset.name
                                        ))
                                        .truncate(),
                                    )
                                    .on_hover_text(&asset.name)
                                })
                                .inner
                            })
                            .inner;
                        crate::qa::register_component_with_metadata(
                            format!("{qa_id}.image_collection.candidate:{}", asset.id),
                            "image_collection_candidate",
                            crate::qa::global_response_rect(ui.ctx(), &candidate),
                            candidate.enabled(),
                            Some(serde_json::json!({
                                "asset_id": asset.id,
                                "name": asset.name,
                                "kind": "image",
                                "selected": false,
                                "collection_full": full,
                            })),
                        );
                        if candidate.clicked() && !full {
                            action = Some(CollectionAction::Add(asset.id));
                        }
                    }
                });
        });

    let response = row.response.union(selector);
    if let Some(payload) = response.dnd_hover_payload::<AuthoringLibraryDrag>() {
        if let AuthoringLibraryDrag::Asset(asset_id) = *payload {
            let eligible = candidates.iter().any(|asset| asset.id == asset_id)
                && !value.assets.contains(&asset_id)
                && value.assets.len() < IMAGE_COLLECTION_MAX_ASSETS;
            ui.painter().rect_stroke(
                response.rect,
                3.0,
                Stroke::new(
                    2.0,
                    if eligible {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().error_fg_color
                    },
                ),
                StrokeKind::Inside,
            );
            crate::qa::register_component_with_metadata(
                format!("{qa_id}.image_collection.drop_target"),
                "image_collection_drop_target",
                crate::qa::global_response_rect(ui.ctx(), &response),
                eligible,
                Some(serde_json::json!({
                    "asset_id": asset_id,
                    "accepts": "image_asset",
                    "eligible": eligible,
                })),
            );
            if response
                .dnd_release_payload::<AuthoringLibraryDrag>()
                .is_some()
            {
                *library_drag = None;
                if eligible {
                    action = Some(CollectionAction::Add(asset_id));
                }
            }
        }
    }
    let changed = action.is_some_and(|action| apply_action(&mut value.assets, action));
    finish(ui, qa_id, value, &candidates, response, changed)
}

fn finish(
    ui: &egui::Ui,
    qa_id: &str,
    value: &ImageCollectionValue,
    candidates: &[&Asset],
    response: Response,
    changed: bool,
) -> ImageCollectionEdit {
    let names = value
        .assets
        .iter()
        .map(|asset_id| {
            candidates
                .iter()
                .find(|asset| asset.id == *asset_id)
                .map(|asset| asset.name.clone())
        })
        .collect::<Vec<_>>();
    crate::qa::register_component_with_metadata(
        format!("{qa_id}.image_collection.selector"),
        "image_collection_selector",
        crate::qa::global_response_rect(ui.ctx(), &response),
        response.enabled(),
        Some(serde_json::json!({
            "asset_ids": value.assets,
            "asset_names": names,
            "count": value.assets.len(),
            "maximum": IMAGE_COLLECTION_MAX_ASSETS,
            "empty_fallback": "analytic_disc",
            "candidate_asset_ids": candidates.iter().map(|asset| asset.id).collect::<Vec<_>>(),
            "changed": changed,
        })),
    );
    ImageCollectionEdit { response, changed }
}

fn image_candidates(project: &AuthoringProject) -> Vec<&Asset> {
    let mut candidates = project
        .assets
        .iter()
        .filter(|asset| asset.kind == AssetKind::Image)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    candidates
}

#[expect(
    clippy::too_many_arguments,
    reason = "the row borrows the shared Project, media-preview service, and editor action owner without duplicating or hiding those responsibilities"
)]
fn selected_asset_row(
    ui: &mut egui::Ui,
    qa_id: &str,
    project: &Arc<AuthoringProject>,
    media_previews: &mut AuthoringMediaPreviewService,
    index: usize,
    asset_id: uuid::Uuid,
    asset: Option<&Asset>,
    count: usize,
    action: &mut Option<CollectionAction>,
) {
    let row = ui.horizontal(|ui| {
        if let Some(asset) = asset {
            asset_thumbnail(
                ui,
                qa_id,
                &format!("selected:{asset_id}"),
                project,
                media_previews,
                asset,
                32.0,
            );
            ui.add_sized([140.0, 32.0], egui::Label::new(&asset.name).truncate())
                .on_hover_text(&asset.name);
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "Missing Image Asset");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let remove = ui.small_button(icons::TRASH).on_hover_text("Remove image");
            register_action(ui, qa_id, asset_id, "remove", &remove, true, index);
            if remove.clicked() {
                *action = Some(CollectionAction::Remove(index));
            }
            let down = ui
                .add_enabled(index + 1 < count, egui::Button::new(icons::ARROW_DOWN))
                .on_hover_text("Move image later");
            register_action(
                ui,
                qa_id,
                asset_id,
                "move_down",
                &down,
                down.enabled(),
                index,
            );
            if down.clicked() {
                *action = Some(CollectionAction::MoveDown(index));
            }
            let up = ui
                .add_enabled(index > 0, egui::Button::new(icons::ARROW_UP))
                .on_hover_text("Move image earlier");
            register_action(ui, qa_id, asset_id, "move_up", &up, up.enabled(), index);
            if up.clicked() {
                *action = Some(CollectionAction::MoveUp(index));
            }
        });
    });
    crate::qa::register_component_with_metadata(
        format!("{qa_id}.image_collection.entry:{asset_id}"),
        "image_collection_entry",
        crate::qa::global_response_rect(ui.ctx(), &row.response),
        true,
        Some(serde_json::json!({
            "asset_id": asset_id,
            "name": asset.map(|asset| asset.name.as_str()),
            "index": index,
        })),
    );
}

fn register_action(
    ui: &egui::Ui,
    qa_id: &str,
    asset_id: uuid::Uuid,
    action: &str,
    response: &Response,
    enabled: bool,
    index: usize,
) {
    crate::qa::register_component_with_metadata(
        format!("{qa_id}.image_collection.entry:{asset_id}.{action}"),
        "image_collection_action",
        crate::qa::global_response_rect(ui.ctx(), response),
        enabled,
        Some(serde_json::json!({
            "asset_id": asset_id,
            "index": index,
            "action": action,
        })),
    );
}

fn apply_action(assets: &mut Vec<uuid::Uuid>, action: CollectionAction) -> bool {
    match action {
        CollectionAction::Add(asset_id)
            if assets.len() < IMAGE_COLLECTION_MAX_ASSETS && !assets.contains(&asset_id) =>
        {
            assets.push(asset_id);
        }
        CollectionAction::Remove(index) if index < assets.len() => {
            assets.remove(index);
        }
        CollectionAction::MoveUp(index) if index > 0 && index < assets.len() => {
            assets.swap(index, index - 1);
        }
        CollectionAction::MoveDown(index) if index + 1 < assets.len() => {
            assets.swap(index, index + 1);
        }
        _ => return false,
    }
    true
}

fn asset_thumbnail(
    ui: &mut egui::Ui,
    qa_id: &str,
    slot: &str,
    project: &Arc<AuthoringProject>,
    media_previews: &mut AuthoringMediaPreviewService,
    asset: &Asset,
    extent: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(extent, extent), Sense::hover());
    ui.painter()
        .rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let evaluation_fps = project
        .timelines
        .get(&project.root_timeline_id)
        .map_or(30.0, |timeline| timeline.fps.to_f64());
    let frame = media_previews.request(
        ui.ctx(),
        Arc::clone(project),
        asset,
        representative_source_time(asset),
        evaluation_fps,
        preview_request_size(ui.ctx(), rect.size()),
    );
    if !paint_media_preview_texture(ui, rect.shrink(1.0), &frame) {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if frame.pending {
                icons::CIRCLE_NOTCH
            } else {
                icons::IMAGE_BROKEN
            },
            egui::FontId::proportional(extent * 0.55),
            ui.visuals().weak_text_color(),
        );
    }
    crate::qa::register_component_with_metadata(
        format!("{qa_id}.image_collection.preview:{slot}"),
        "image_collection_asset_preview",
        crate::qa::global_response_rect(ui.ctx(), &response),
        true,
        Some(serde_json::json!({
            "asset_id": asset.id,
            "name": asset.name,
            "ready": frame.texture.is_some(),
            "pending": frame.pending,
            "fallback": frame.fallback,
            "content_hash": frame.content_hash,
            "uses_shared_media_cache": true,
        })),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_actions_preserve_order_and_identity() {
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        let third = uuid::Uuid::new_v4();
        let mut assets = vec![first, second];
        assert!(apply_action(&mut assets, CollectionAction::Add(third)));
        assert!(apply_action(&mut assets, CollectionAction::MoveUp(2)));
        assert_eq!(assets, [first, third, second]);
        assert!(apply_action(&mut assets, CollectionAction::MoveDown(0)));
        assert_eq!(assets, [third, first, second]);
        assert!(apply_action(&mut assets, CollectionAction::Remove(1)));
        assert_eq!(assets, [third, second]);
        assert!(!apply_action(&mut assets, CollectionAction::Add(third)));
        assert!(!apply_action(&mut assets, CollectionAction::MoveUp(0)));
        assert!(!apply_action(&mut assets, CollectionAction::Remove(4)));
    }
}
