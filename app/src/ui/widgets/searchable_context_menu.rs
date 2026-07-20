use egui::{Key, Pos2, Rect, ScrollArea, TextEdit, Ui, Vec2};
use std::collections::BTreeMap;

pub const DEFAULT_SEARCHABLE_RESULTS_MAX_HEIGHT: f32 = 300.0;
pub const SEARCHABLE_POPUP_VIEWPORT_MARGIN: f32 = 8.0;

/// Screen-space placement shared by searchable popup callers.
///
/// `area_anchor` and `pivot` keep the popup attached to the requested pointer
/// edge while `max_height` bounds its usable height inside the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchablePopupPlacement {
    pub area_anchor: Pos2,
    pub pivot: egui::Align2,
    pub width: f32,
    pub max_height: f32,
    pub opens_upward: bool,
    pub clamped: bool,
}

/// Place a popup at `desired_anchor`, preferring the space below the anchor
/// unless the requested root size would be clipped and more room is available
/// above. The result is inset from the viewport edges and horizontally
/// clamped. Apply its `pivot` and `area_anchor` to `egui::Area`, then render the
/// contents with [`show_searchable_popup_frame`].
#[must_use]
pub fn searchable_popup_placement(
    desired_anchor: Pos2,
    desired_popup_size: Vec2,
    viewport: Rect,
) -> SearchablePopupPlacement {
    let margin = SEARCHABLE_POPUP_VIEWPORT_MARGIN;
    let left = viewport.left() + margin;
    let right = (viewport.right() - margin).max(left);
    let top = viewport.top() + margin;
    let bottom = (viewport.bottom() - margin).max(top);
    let available_width = (right - left).max(0.0);
    let width = finite_non_negative(desired_popup_size.x).min(available_width);
    let anchor_x = finite_or(desired_anchor.x, left).clamp(left, right);
    let anchor_y = finite_or(desired_anchor.y, top).clamp(top, bottom);
    let space_above = (anchor_y - top).max(0.0);
    let space_below = (bottom - anchor_y).max(0.0);
    let desired_height = finite_non_negative(desired_popup_size.y);
    let opens_upward = desired_height > space_below && space_above > space_below;
    let max_height = desired_height.min(if opens_upward {
        space_above
    } else {
        space_below
    });
    let x = anchor_x.clamp(left, (right - width).max(left));
    let clamped =
        !desired_anchor.is_finite() || x != desired_anchor.x || anchor_y != desired_anchor.y;

    SearchablePopupPlacement {
        area_anchor: Pos2::new(x, anchor_y),
        pivot: if opens_upward {
            egui::Align2::LEFT_BOTTOM
        } else {
            egui::Align2::LEFT_TOP
        },
        width,
        max_height,
        opens_upward,
        clamped,
    }
}

/// Render a menu frame whose outer root respects the placement bounds.
pub fn show_searchable_popup_frame<R>(
    ui: &mut Ui,
    placement: SearchablePopupPlacement,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let frame = egui::Frame::menu(ui.style());
    let margin = frame.total_margin().sum();
    frame.show(ui, |ui| {
        ui.set_width((placement.width - margin.x).max(0.0));
        ui.set_max_height((placement.max_height - margin.y).max(0.0));
        add_contents(ui)
    })
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// One entry in a searchable menu.
///
/// `keywords` contain aliases that should find the item without being shown in
/// the menu. Disabled items remain visible, but mouse and keyboard selection
/// both skip them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchableItem<T> {
    pub label: String,
    pub category: Option<String>,
    pub keywords: Vec<String>,
    pub enabled: bool,
    /// Stable QA bridge identifier for the real egui button, when the menu is
    /// used by a coordinate-driven integration test.
    pub qa_id: Option<String>,
    /// Caller-defined action data exposed alongside `qa_id`.
    pub qa_metadata: Option<serde_json::Value>,
    pub value: T,
}

impl<T> SearchableItem<T> {
    pub fn new(label: impl Into<String>, value: T) -> Self {
        Self {
            label: label.into(),
            category: None,
            keywords: Vec::new(),
            enabled: true,
            qa_id: None,
            qa_metadata: None,
            value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchNavigation {
    Previous,
    Next,
    First,
    Last,
}

#[derive(Clone, Default)]
struct MenuState {
    query: String,
    /// Original item index, not an index into a transient filtered list.
    selected_item: Option<usize>,
    /// A one-shot request. Keeping this separate from `selected_item` lets a
    /// user scroll freely without the selected row snapping back every frame.
    scroll_to: Option<usize>,
}

impl MenuState {
    fn set_selection(&mut self, selection: Option<usize>, request_scroll: bool) {
        if self.selected_item == selection {
            return;
        }
        self.selected_item = selection;
        self.scroll_to = if request_scroll { selection } else { None };
    }

    fn take_scroll_request(&mut self) -> Option<usize> {
        self.scroll_to.take()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuContents {
    Categories(CategoryNode),
    FlatSearch(Vec<usize>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CategoryNode {
    items: Vec<usize>,
    children: BTreeMap<String, CategoryNode>,
}

/// Return whether the current pointer click is outside the caller-owned popup.
/// Inline category accordions remain inside this rectangle at every depth.
#[must_use]
pub fn searchable_menu_click_is_outside(
    ctx: &egui::Context,
    _id_source: &str,
    root_rect: Rect,
) -> bool {
    ctx.input(|input| input.pointer.interact_pos())
        .is_some_and(|pointer| !root_rect.contains(pointer))
}

/// Register a searchable popup's requested anchor and real root geometry.
pub fn register_searchable_popup_qa(
    qa_id: &str,
    requested_anchor: Pos2,
    placement: SearchablePopupPlacement,
    actual_root_rect: Rect,
) {
    let actual = actual_root_rect.min;
    let anchored_y = if placement.opens_upward {
        actual_root_rect.bottom()
    } else {
        actual_root_rect.top()
    };
    crate::qa::register_component_with_metadata(
        qa_id,
        "searchable_context_menu",
        actual_root_rect,
        true,
        Some(serde_json::json!({
            "requested_anchor": {"x": requested_anchor.x, "y": requested_anchor.y},
            "placement_anchor": {"x": placement.area_anchor.x, "y": placement.area_anchor.y},
            "pivot": if placement.opens_upward { "left_bottom" } else { "left_top" },
            "actual_root_rect": {
                "min_x": actual_root_rect.min.x,
                "min_y": actual_root_rect.min.y,
                "max_x": actual_root_rect.max.x,
                "max_y": actual_root_rect.max.y,
                "width": actual_root_rect.width(),
                "height": actual_root_rect.height(),
            },
            "offset": {"x": actual.x - requested_anchor.x, "y": actual.y - requested_anchor.y},
            "anchor_edge_offset": {
                "x": actual_root_rect.left() - requested_anchor.x,
                "y": anchored_y - requested_anchor.y,
            },
            "clamped": placement.clamped,
            "opens_upward": placement.opens_upward,
        })),
    );
}

/// Return the indices whose label or keyword matches every whitespace-separated
/// query term. Matching is case-insensitive and preserves input order.
pub fn filter_searchable_items<T>(items: &[SearchableItem<T>], query: &str) -> Vec<usize> {
    let terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();

    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            if terms.is_empty() {
                return true;
            }
            let label = item.label.to_lowercase();
            let keywords = item
                .keywords
                .iter()
                .map(|keyword| keyword.to_lowercase())
                .collect::<Vec<_>>();
            terms.iter().all(|term| {
                label.contains(term) || keywords.iter().any(|keyword| keyword.contains(term))
            })
        })
        .map(|(index, _)| index)
        .collect()
}

/// Move a selection through visible enabled items without wrapping.
///
/// The returned value is an original item index. Keeping that identity avoids
/// selecting a different entry when a query changes the filtered ordering.
pub fn navigate_searchable_items<T>(
    items: &[SearchableItem<T>],
    visible_indices: &[usize],
    selected_item: Option<usize>,
    navigation: SearchNavigation,
) -> Option<usize> {
    let selectable = visible_indices
        .iter()
        .copied()
        .filter(|index| items.get(*index).is_some_and(|item| item.enabled))
        .collect::<Vec<_>>();
    let first = selectable.first().copied()?;
    let last = selectable.last().copied()?;
    let current = selected_item.and_then(|selected| {
        selectable
            .iter()
            .position(|candidate| *candidate == selected)
    });

    match navigation {
        SearchNavigation::First => Some(first),
        SearchNavigation::Last => Some(last),
        SearchNavigation::Next => current.map_or(Some(first), |index| {
            selectable
                .get(index.saturating_add(1))
                .copied()
                .or(Some(last))
        }),
        SearchNavigation::Previous => current.map_or(Some(last), |index| {
            index
                .checked_sub(1)
                .and_then(|index| selectable.get(index).copied())
                .or(Some(first))
        }),
    }
}

/// Render a categorized searchable menu while exposing its actual search box
/// and item buttons to the loopback QA bridge. Input is still delivered via
/// egui; these identifiers do not provide a model-mutation shortcut.
pub fn show_searchable_items_with_qa<T: Clone>(
    ui: &mut Ui,
    id_source: &str,
    qa_search_id: Option<&str>,
    items: &[SearchableItem<T>],
) -> Option<T> {
    let id = ui.make_persistent_id(id_source);
    let mut state = ui.data_mut(|data| data.get_temp::<MenuState>(id).unwrap_or_default());

    let text_response = ui.add(TextEdit::singleline(&mut state.query).hint_text("Search..."));
    if let Some(qa_search_id) = qa_search_id {
        crate::qa::register_component_with_metadata(
            qa_search_id,
            "searchable_menu_query",
            text_response.rect,
            text_response.enabled(),
            Some(serde_json::json!({"action": "filter"})),
        );
    }
    if state.query.is_empty() && !ui.memory(|memory| memory.has_focus(text_response.id)) {
        text_response.request_focus();
    }

    if ui.input(|input| input.key_pressed(Key::Escape)) {
        ui.data_mut(|data| data.insert_temp(id, MenuState::default()));
        ui.close();
        return None;
    }

    ui.separator();

    let contents = menu_contents(items, &state.query);
    let keyboard_selection = match &contents {
        MenuContents::Categories(_) => {
            // Native `menu_button` owns pointer and keyboard navigation while
            // browsing. The custom selection state is reserved for flat search.
            state.set_selection(None, false);
            None
        }
        MenuContents::FlatSearch(displayed) => {
            let selection_is_invalid = state.selected_item.is_none_or(|selected| {
                !displayed.contains(&selected)
                    || items.get(selected).is_none_or(|item| !item.enabled)
            });
            if text_response.changed() {
                let selection =
                    navigate_searchable_items(items, displayed, None, SearchNavigation::First);
                state.set_selection(selection, true);
            } else if selection_is_invalid {
                let selection =
                    navigate_searchable_items(items, displayed, None, SearchNavigation::First);
                state.set_selection(selection, false);
            }

            for (key, navigation) in [
                (Key::ArrowDown, SearchNavigation::Next),
                (Key::ArrowUp, SearchNavigation::Previous),
                (Key::Home, SearchNavigation::First),
                (Key::End, SearchNavigation::Last),
            ] {
                if ui.input(|input| input.key_pressed(key)) {
                    let selection = navigate_searchable_items(
                        items,
                        displayed,
                        state.selected_item,
                        navigation,
                    );
                    state.set_selection(selection, true);
                }
            }

            ui.input(|input| input.key_pressed(Key::Enter))
                .then_some(state.selected_item)
                .flatten()
        }
    };

    let mut clicked_selection = None;
    let scroll_to = state.take_scroll_request();

    let results_height = DEFAULT_SEARCHABLE_RESULTS_MAX_HEIGHT.min(ui.available_height().max(0.0));
    ScrollArea::vertical()
        .id_salt(("searchable_menu_results", id))
        .max_height(results_height)
        .show(ui, |ui| match &contents {
            MenuContents::Categories(root) if root.is_empty() => {
                ui.label("No results");
            }
            MenuContents::Categories(root) => render_category_node(
                ui,
                id_source,
                root,
                &[],
                items,
                qa_search_id,
                &mut clicked_selection,
            ),
            MenuContents::FlatSearch(displayed) if displayed.is_empty() => {
                ui.label("No results");
            }
            MenuContents::FlatSearch(displayed) => {
                for index in displayed.iter().copied() {
                    if render_item_button(
                        ui,
                        index,
                        items,
                        state.selected_item == Some(index),
                        scroll_to == Some(index),
                    ) {
                        clicked_selection = Some(index);
                    }
                }
            }
        });

    let selection = clicked_selection.or(keyboard_selection).and_then(|index| {
        items
            .get(index)
            .filter(|item| item.enabled)
            .map(|item| item.value.clone())
    });

    if selection.is_some() {
        state = MenuState::default();
        ui.close();
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    selection
}

impl CategoryNode {
    fn is_empty(&self) -> bool {
        self.items.is_empty() && self.children.is_empty()
    }
}

fn menu_contents<T>(items: &[SearchableItem<T>], query: &str) -> MenuContents {
    let filtered = filter_searchable_items(items, query);
    if query.trim().is_empty() {
        MenuContents::Categories(category_tree(items, &filtered))
    } else {
        MenuContents::FlatSearch(filtered)
    }
}

fn category_tree<T>(items: &[SearchableItem<T>], filtered: &[usize]) -> CategoryNode {
    let mut root = CategoryNode::default();
    for index in filtered.iter().copied() {
        let Some(item) = items.get(index) else {
            continue;
        };
        let path = normalized_category_path(item.category.as_deref());
        if path.is_empty() {
            root.items.push(index);
        } else {
            let mut node = &mut root;
            for segment in path {
                node = node.children.entry(segment).or_default();
            }
            node.items.push(index);
        }
    }
    root
}

fn normalized_category_path(category: Option<&str>) -> Vec<String> {
    category
        .into_iter()
        .flat_map(|category| category.split('/'))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

fn render_category_node<T>(
    ui: &mut Ui,
    id_source: &str,
    node: &CategoryNode,
    path: &[String],
    items: &[SearchableItem<T>],
    qa_search_id: Option<&str>,
    clicked_selection: &mut Option<usize>,
) {
    for (label, child) in &node.children {
        let mut child_path = path.to_vec();
        child_path.push(label.clone());
        let accordion = egui::CollapsingHeader::new(label)
            .id_salt(("searchable_menu_category", id_source, child_path.join("/")))
            .default_open(false)
            .show(ui, |ui| {
                render_category_node(
                    ui,
                    id_source,
                    child,
                    &child_path,
                    items,
                    qa_search_id,
                    clicked_selection,
                );
            });
        if let Some(qa_search_id) = qa_search_id {
            crate::qa::register_component_with_metadata(
                format!("{qa_search_id}.category:{}", child_path.join("/")),
                "searchable_menu_category",
                accordion.header_response.rect,
                accordion.header_response.enabled(),
                Some(serde_json::json!({
                    "action": "toggle_category",
                    "category_path": child_path,
                    "inline": true,
                    "open": accordion.body_response.is_some(),
                })),
            );
        }
    }
    if !node.children.is_empty() && !node.items.is_empty() {
        ui.separator();
    }

    for index in node.items.iter().copied() {
        if render_item_button(ui, index, items, false, false) {
            *clicked_selection = Some(index);
        }
    }
}

fn render_item_button<T>(
    ui: &mut Ui,
    index: usize,
    items: &[SearchableItem<T>],
    selected: bool,
    scroll_to: bool,
) -> bool {
    let Some(item) = items.get(index) else {
        return false;
    };
    let response = ui.add_enabled(
        item.enabled,
        egui::Button::selectable(selected, &item.label).frame(false),
    );
    if let Some(qa_id) = &item.qa_id {
        let mut metadata = item.qa_metadata.clone().unwrap_or_else(|| {
            serde_json::json!({
                "label": item.label,
                "category": item.category,
            })
        });
        if let Some(object) = metadata.as_object_mut() {
            object
                .entry("label")
                .or_insert_with(|| serde_json::json!(item.label));
            object
                .entry("category")
                .or_insert_with(|| serde_json::json!(item.category));
        }
        crate::qa::register_component_with_metadata(
            qa_id,
            "searchable_menu_item",
            response.rect,
            response.enabled(),
            Some(metadata),
        );
    }
    if scroll_to {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<SearchableItem<u8>> {
        fn item(
            label: &str,
            category: Option<&str>,
            keywords: &[&str],
            enabled: bool,
            value: u8,
        ) -> SearchableItem<u8> {
            SearchableItem {
                label: label.to_string(),
                category: category.map(str::to_string),
                keywords: keywords
                    .iter()
                    .map(|keyword| (*keyword).to_string())
                    .collect(),
                enabled,
                qa_id: None,
                qa_metadata: None,
                value,
            }
        }

        vec![
            item(
                "Gaussian Blur",
                Some("Effect / Image"),
                &["soften", "defocus"],
                true,
                1,
            ),
            item(
                "Merge",
                Some("Compositing / Merge"),
                &["blend", "layers"],
                true,
                2,
            ),
            item(
                "Unavailable Effect",
                Some("Effect / Image"),
                &["offline"],
                false,
                3,
            ),
            item("Text", None, &["title", "caption"], true, 4),
        ]
    }

    #[test]
    fn filter_matches_case_insensitive_labels_and_keywords() {
        let items = fixture();
        assert_eq!(filter_searchable_items(&items, "gAuSs"), vec![0]);
        assert_eq!(filter_searchable_items(&items, "DEFOCUS"), vec![0]);
        assert_eq!(filter_searchable_items(&items, "blend LAYERS"), vec![1]);
        assert_eq!(filter_searchable_items(&items, "title"), vec![3]);
        assert!(filter_searchable_items(&items, "missing").is_empty());
    }

    #[test]
    fn empty_query_builds_sorted_recursive_category_accordions() {
        let items = fixture();
        let MenuContents::Categories(root) = menu_contents(&items, "") else {
            panic!("an empty query must browse inline category accordions")
        };

        assert_eq!(root.items, vec![3]);
        assert_eq!(
            root.children.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["Compositing", "Effect"]
        );
        assert_eq!(
            root.children["Compositing"].children["Merge"].items,
            vec![1]
        );
        assert_eq!(root.children["Effect"].children["Image"].items, vec![0, 2]);

        assert_eq!(
            menu_contents(&items, "  \t"),
            MenuContents::Categories(root)
        );
    }

    #[test]
    fn non_empty_query_flattens_matches_in_catalog_order() {
        let items = fixture();
        assert_eq!(
            menu_contents(&items, "e"),
            MenuContents::FlatSearch(vec![0, 1, 2, 3])
        );
        assert_eq!(
            menu_contents(&items, "defocus"),
            MenuContents::FlatSearch(vec![0])
        );
    }

    #[test]
    fn outside_click_geometry_uses_the_inline_root_rect() {
        let root = Rect::from_min_max(Pos2::new(1_300.0, 502.0), Pos2::new(1_620.0, 664.0));
        assert!(root.contains(Pos2::new(1_325.0, 633.0)));
        assert!(!root.contains(Pos2::new(1_416.0, 742.0)));
        assert!(!root.contains(Pos2::new(1_000.0, 820.0)));
    }

    #[test]
    fn keyboard_navigation_skips_disabled_items_and_clamps_at_ends() {
        let items = fixture();
        let visible = vec![0, 2, 3];

        assert_eq!(
            navigate_searchable_items(&items, &visible, None, SearchNavigation::First),
            Some(0)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, Some(0), SearchNavigation::Next),
            Some(3)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, Some(3), SearchNavigation::Next),
            Some(3)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, Some(3), SearchNavigation::Previous),
            Some(0)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, None, SearchNavigation::Last),
            Some(3)
        );
    }

    #[test]
    fn no_enabled_result_has_no_keyboard_selection() {
        let items = fixture();
        assert_eq!(
            navigate_searchable_items(&items, &[2], None, SearchNavigation::First),
            None
        );
    }

    #[test]
    fn scroll_request_is_only_emitted_once_for_an_actual_selection_change() {
        let mut state = MenuState {
            selected_item: Some(0),
            ..MenuState::default()
        };

        state.set_selection(Some(1), true);
        assert_eq!(state.take_scroll_request(), Some(1));
        assert_eq!(state.take_scroll_request(), None);

        state.set_selection(Some(1), true);
        assert_eq!(state.take_scroll_request(), None);

        state.set_selection(Some(0), false);
        assert_eq!(state.take_scroll_request(), None);
    }

    #[test]
    fn stable_scroll_area_id_preserves_manual_offset_across_frames() {
        let items = (0_u8..40)
            .map(|value| SearchableItem::new(format!("Item {value:02}"), value))
            .collect::<Vec<_>>();
        let context = egui::Context::default();

        fn render_frame(
            context: &egui::Context,
            items: &[SearchableItem<u8>],
            time: f64,
        ) -> egui::Id {
            let mut scroll_id = None;
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 500.0))),
                    time: Some(time),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let menu_id = ui.make_persistent_id("scroll-preservation-test");
                        scroll_id = Some(ui.make_persistent_id(egui::Id::new((
                            "searchable_menu_results",
                            menu_id,
                        ))));
                        let _selection = show_searchable_items_with_qa(
                            ui,
                            "scroll-preservation-test",
                            None,
                            items,
                        );
                    });
                },
            ));
            scroll_id.expect("the scroll area is rendered")
        }

        let scroll_id = render_frame(&context, &items, 0.0);
        let mut scroll_state = egui::containers::scroll_area::State::load(&context, scroll_id)
            .expect("scroll state exists after the first frame");
        scroll_state.offset.y = 120.0;
        scroll_state.store(&context, scroll_id);

        assert_eq!(render_frame(&context, &items, 1.0 / 60.0), scroll_id);
        let first_offset = egui::containers::scroll_area::State::load(&context, scroll_id)
            .expect("scroll state survives")
            .offset
            .y;
        assert!((first_offset - 120.0).abs() < f32::EPSILON);

        assert_eq!(render_frame(&context, &items, 2.0 / 60.0), scroll_id);
        let second_offset = egui::containers::scroll_area::State::load(&context, scroll_id)
            .expect("scroll state survives another frame")
            .offset
            .y;
        assert!((second_offset - 120.0).abs() < f32::EPSILON);
    }

    #[test]
    fn bottom_edge_popup_opens_upward_at_full_desired_height() {
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let placement =
            searchable_popup_placement(Pos2::new(780.0, 580.0), Vec2::new(300.0, 320.0), viewport);

        assert!(placement.opens_upward);
        assert!(placement.clamped);
        assert_eq!(placement.area_anchor, Pos2::new(492.0, 580.0));
        assert_eq!(placement.pivot, egui::Align2::LEFT_BOTTOM);
        let size = Vec2::new(placement.width, placement.max_height);
        let maximum_rect = Rect::from_min_size(placement.area_anchor - Vec2::Y * size.y, size);
        assert_eq!(size, Vec2::new(300.0, 320.0));
        assert!(viewport.contains(maximum_rect.min));
        assert!(viewport.contains(maximum_rect.max));
    }

    #[test]
    fn popup_height_uses_larger_side_when_neither_side_fits() {
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let placement = searchable_popup_placement(
            Pos2::new(100.0, 500.0),
            Vec2::new(260.0, 1_000.0),
            viewport,
        );

        assert!(placement.opens_upward);
        assert_eq!(placement.area_anchor.y - placement.max_height, 8.0);
        assert_eq!(placement.max_height, 492.0);
    }

    #[test]
    fn popup_keeps_an_unclamped_requested_anchor() {
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(1_200.0, 800.0));
        let anchor = Pos2::new(240.0, 180.0);
        let placement = searchable_popup_placement(anchor, Vec2::new(320.0, 348.0), viewport);

        assert_eq!(placement.area_anchor, anchor);
        assert_eq!(placement.pivot, egui::Align2::LEFT_TOP);
        assert!(!placement.opens_upward);
        assert!(!placement.clamped);
    }
}
