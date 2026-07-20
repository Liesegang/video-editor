use egui::{Key, Pos2, Rect, ScrollArea, TextEdit, Ui, Vec2};
use std::collections::BTreeMap;

pub const DEFAULT_SEARCHABLE_RESULTS_MAX_HEIGHT: f32 = 300.0;
pub const SEARCHABLE_POPUP_VIEWPORT_MARGIN: f32 = 8.0;

/// Screen-space placement shared by searchable popup callers.
///
/// `position` is the popup's top-left corner and `max_height` is the usable
/// popup height after clamping the desired content to the viewport. Callers
/// should apply both values: using only the height can still leave a popup
/// clipped against the bottom edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchablePopupPlacement {
    pub position: Pos2,
    pub width: f32,
    pub max_height: f32,
    pub opens_upward: bool,
}

/// Place a popup at `desired_anchor`, preferring the space below the anchor
/// unless the requested content would be clipped and more room is available
/// above. The result is inset from the viewport edges and horizontally
/// clamped, so it is suitable for `egui::Area::fixed_pos` plus a matching
/// `Ui::set_max_height`.
#[must_use]
pub fn searchable_popup_placement(
    desired_anchor: Pos2,
    desired_content_size: Vec2,
    viewport: Rect,
) -> SearchablePopupPlacement {
    let margin = SEARCHABLE_POPUP_VIEWPORT_MARGIN;
    let left = viewport.left() + margin;
    let right = (viewport.right() - margin).max(left);
    let top = viewport.top() + margin;
    let bottom = (viewport.bottom() - margin).max(top);
    let available_width = (right - left).max(0.0);
    let width = finite_non_negative(desired_content_size.x).min(available_width);
    let anchor_x = finite_or(desired_anchor.x, left).clamp(left, right);
    let anchor_y = finite_or(desired_anchor.y, top).clamp(top, bottom);
    let space_above = (anchor_y - top).max(0.0);
    let space_below = (bottom - anchor_y).max(0.0);
    let desired_height = finite_non_negative(desired_content_size.y);
    let opens_upward = desired_height > space_below && space_above > space_below;
    let max_height = desired_height.min(if opens_upward {
        space_above
    } else {
        space_below
    });
    let x = anchor_x.clamp(left, (right - width).max(left));
    let y = if opens_upward {
        anchor_y - max_height
    } else {
        anchor_y
    };

    SearchablePopupPlacement {
        position: Pos2::new(x, y),
        width,
        max_height,
        opens_upward,
    }
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
    browse_path: Vec<String>,
    selected: Option<MenuSelection>,
    /// A one-shot request. Keeping this separate from `selected` lets a
    /// user scroll freely without the selected row snapping back every frame.
    scroll_to: Option<MenuSelection>,
}

impl MenuState {
    fn set_selection(&mut self, selection: Option<MenuSelection>, request_scroll: bool) {
        if self.selected == selection {
            return;
        }
        self.selected = selection.clone();
        self.scroll_to = if request_scroll { selection } else { None };
    }

    fn take_scroll_request(&mut self) -> Option<MenuSelection> {
        self.scroll_to.take()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuSelection {
    Back,
    Category(Vec<String>),
    /// Original item index, not an index into a transient filtered list.
    Item(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuRow {
    Back { label: String },
    Category { label: String, path: Vec<String> },
    Item { index: usize },
}

impl MenuRow {
    fn selection(&self) -> MenuSelection {
        match self {
            Self::Back { .. } => MenuSelection::Back,
            Self::Category { path, .. } => MenuSelection::Category(path.clone()),
            Self::Item { index } => MenuSelection::Item(*index),
        }
    }
}

#[derive(Default)]
struct CategoryNode {
    items: Vec<usize>,
    children: BTreeMap<String, CategoryNode>,
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

    let filtered = filter_searchable_items(items, &state.query);
    let browse_mode = state.query.trim().is_empty();
    if browse_mode && !category_path_exists(items, &filtered, &state.browse_path) {
        state.browse_path.clear();
        state.set_selection(None, false);
    }
    if browse_mode
        && !state.browse_path.is_empty()
        && ui.input(|input| input.key_pressed(Key::ArrowLeft))
    {
        state.browse_path.pop();
        state.set_selection(None, false);
    }

    let rows = menu_rows(items, &filtered, &state.query, &state.browse_path);
    let selection_is_invalid = state.selected.as_ref().is_none_or(|selected| {
        !rows
            .iter()
            .any(|row| row.selection() == *selected && row_is_enabled(row, items))
    });
    if text_response.changed() {
        let selection = navigate_menu_rows(items, &rows, None, SearchNavigation::First);
        state.set_selection(selection, true);
    } else if selection_is_invalid {
        let selection = navigate_menu_rows(items, &rows, None, SearchNavigation::First);
        state.set_selection(selection, false);
    }

    for (key, navigation) in [
        (Key::ArrowDown, SearchNavigation::Next),
        (Key::ArrowUp, SearchNavigation::Previous),
        (Key::Home, SearchNavigation::First),
        (Key::End, SearchNavigation::Last),
    ] {
        if ui.input(|input| input.key_pressed(key)) {
            let selection = navigate_menu_rows(items, &rows, state.selected.as_ref(), navigation);
            state.set_selection(selection, true);
        }
    }

    let (enter_pressed, right_pressed) = ui.input(|input| {
        (
            input.key_pressed(Key::Enter),
            input.key_pressed(Key::ArrowRight),
        )
    });
    let keyboard_activation = if enter_pressed {
        state.selected.clone()
    } else if browse_mode && right_pressed {
        state
            .selected
            .as_ref()
            .filter(|selection| matches!(selection, MenuSelection::Category(_)))
            .cloned()
    } else {
        None
    };
    let mut clicked_activation = None;
    let scroll_to = state.take_scroll_request();

    let results_height = DEFAULT_SEARCHABLE_RESULTS_MAX_HEIGHT.min(ui.available_height().max(0.0));
    ScrollArea::vertical()
        .id_salt(("searchable_menu_results", id))
        .max_height(results_height)
        .show(ui, |ui| {
            if rows.is_empty() {
                ui.label("No results");
                return;
            }

            for row in &rows {
                let selection = row.selection();
                let selected = state.selected.as_ref() == Some(&selection);
                let response = match row {
                    MenuRow::Back { label } => {
                        let response = ui.add(
                            egui::Button::selectable(selected, format!("← {label}")).frame(false),
                        );
                        if let Some(qa_search_id) = qa_search_id {
                            crate::qa::register_component_with_metadata(
                                format!("{qa_search_id}.back"),
                                "searchable_menu_category_back",
                                response.rect,
                                response.enabled(),
                                Some(serde_json::json!({
                                    "action": "leave_category",
                                    "category_path": state.browse_path,
                                })),
                            );
                        }
                        response
                    }
                    MenuRow::Category { label, path } => {
                        let response = ui.add(
                            egui::Button::selectable(selected, format!("{label}  ›")).frame(false),
                        );
                        if let Some(qa_search_id) = qa_search_id {
                            crate::qa::register_component_with_metadata(
                                format!("{qa_search_id}.category:{}", path.join("/")),
                                "searchable_menu_category",
                                response.rect,
                                response.enabled(),
                                Some(serde_json::json!({
                                    "action": "enter_category",
                                    "category_path": path,
                                })),
                            );
                        }
                        response
                    }
                    MenuRow::Item { index } => {
                        let Some(item) = items.get(*index) else {
                            continue;
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
                        response
                    }
                };
                if response.clicked() {
                    clicked_activation = Some(selection.clone());
                }
                if scroll_to.as_ref() == Some(&selection) {
                    response.scroll_to_me(Some(egui::Align::Center));
                }
            }
        });

    let activation = clicked_activation.or(keyboard_activation);
    let selection = match activation {
        Some(MenuSelection::Item(index)) => items
            .get(index)
            .filter(|item| item.enabled)
            .map(|item| item.value.clone()),
        Some(selection @ (MenuSelection::Category(_) | MenuSelection::Back)) if browse_mode => {
            enter_browse_selection(&mut state, selection);
            None
        }
        Some(MenuSelection::Category(_) | MenuSelection::Back) | None => None,
    };

    if selection.is_some() {
        state = MenuState::default();
        ui.close();
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    selection
}

fn enter_browse_selection(state: &mut MenuState, selection: MenuSelection) {
    match selection {
        MenuSelection::Category(path) => state.browse_path = path,
        MenuSelection::Back => {
            state.browse_path.pop();
        }
        MenuSelection::Item(_) => return,
    }
    state.set_selection(None, false);
}

fn menu_rows<T>(
    items: &[SearchableItem<T>],
    filtered: &[usize],
    query: &str,
    browse_path: &[String],
) -> Vec<MenuRow> {
    if !query.trim().is_empty() {
        return filtered
            .iter()
            .copied()
            .map(|index| MenuRow::Item { index })
            .collect();
    }

    browse_rows(items, filtered, browse_path)
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

fn category_node_at_path<'a>(root: &'a CategoryNode, path: &[String]) -> Option<&'a CategoryNode> {
    let mut node = root;
    for segment in path {
        node = node.children.get(segment)?;
    }
    Some(node)
}

fn category_path_exists<T>(
    items: &[SearchableItem<T>],
    filtered: &[usize],
    path: &[String],
) -> bool {
    let root = category_tree(items, filtered);
    category_node_at_path(&root, path).is_some()
}

fn browse_rows<T>(
    items: &[SearchableItem<T>],
    filtered: &[usize],
    browse_path: &[String],
) -> Vec<MenuRow> {
    let root = category_tree(items, filtered);
    let Some(node) = category_node_at_path(&root, browse_path) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if !browse_path.is_empty() {
        rows.push(MenuRow::Back {
            label: browse_path
                .last()
                .cloned()
                .unwrap_or_else(|| "Back".to_owned()),
        });
    }
    rows.extend(node.children.keys().map(|label| {
        let mut path = browse_path.to_vec();
        path.push(label.clone());
        MenuRow::Category {
            label: label.clone(),
            path,
        }
    }));
    rows.extend(
        node.items
            .iter()
            .copied()
            .map(|index| MenuRow::Item { index }),
    );
    rows
}

fn row_is_enabled<T>(row: &MenuRow, items: &[SearchableItem<T>]) -> bool {
    match row {
        MenuRow::Back { .. } | MenuRow::Category { .. } => true,
        MenuRow::Item { index } => items.get(*index).is_some_and(|item| item.enabled),
    }
}

fn navigate_menu_rows<T>(
    items: &[SearchableItem<T>],
    rows: &[MenuRow],
    selected: Option<&MenuSelection>,
    navigation: SearchNavigation,
) -> Option<MenuSelection> {
    if rows.iter().all(|row| matches!(row, MenuRow::Item { .. })) {
        let visible_indices = rows
            .iter()
            .filter_map(|row| match row {
                MenuRow::Item { index } => Some(*index),
                MenuRow::Back { .. } | MenuRow::Category { .. } => None,
            })
            .collect::<Vec<_>>();
        let selected_item = match selected {
            Some(MenuSelection::Item(index)) => Some(*index),
            Some(MenuSelection::Back | MenuSelection::Category(_)) | None => None,
        };
        return navigate_searchable_items(items, &visible_indices, selected_item, navigation)
            .map(MenuSelection::Item);
    }

    let selectable = rows
        .iter()
        .filter(|row| row_is_enabled(row, items))
        .map(MenuRow::selection)
        .collect::<Vec<_>>();
    let first = selectable.first()?.clone();
    let last = selectable.last()?.clone();
    let current = selected.and_then(|selected| {
        selectable
            .iter()
            .position(|candidate| candidate == selected)
    });

    match navigation {
        SearchNavigation::First => Some(first),
        SearchNavigation::Last => Some(last),
        SearchNavigation::Next => current.map_or(Some(first), |index| {
            selectable
                .get(index.saturating_add(1))
                .cloned()
                .or(Some(last))
        }),
        SearchNavigation::Previous => current.map_or(Some(last), |index| {
            index
                .checked_sub(1)
                .and_then(|index| selectable.get(index).cloned())
                .or(Some(first))
        }),
    }
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
    fn browse_mode_is_a_sorted_navigable_hierarchy_and_hides_descendants() {
        let items = fixture();
        let filtered = filter_searchable_items(&items, "");
        assert_eq!(
            menu_rows(&items, &filtered, "", &[]),
            vec![
                MenuRow::Category {
                    label: "Compositing".to_owned(),
                    path: vec!["Compositing".to_owned()],
                },
                MenuRow::Category {
                    label: "Effect".to_owned(),
                    path: vec!["Effect".to_owned()],
                },
                MenuRow::Item { index: 3 },
            ]
        );

        let effect_path = vec!["Effect".to_owned()];
        let mut state = MenuState::default();
        enter_browse_selection(&mut state, MenuSelection::Category(effect_path.clone()));
        assert_eq!(state.browse_path, effect_path);
        assert_eq!(
            menu_rows(&items, &filtered, "", &state.browse_path),
            vec![
                MenuRow::Back {
                    label: "Effect".to_owned(),
                },
                MenuRow::Category {
                    label: "Image".to_owned(),
                    path: vec!["Effect".to_owned(), "Image".to_owned()],
                },
            ]
        );

        let image_path = vec!["Effect".to_owned(), "Image".to_owned()];
        enter_browse_selection(&mut state, MenuSelection::Category(image_path.clone()));
        assert_eq!(state.browse_path, image_path);
        assert_eq!(
            menu_rows(&items, &filtered, "", &state.browse_path),
            vec![
                MenuRow::Back {
                    label: "Image".to_owned(),
                },
                MenuRow::Item { index: 0 },
                MenuRow::Item { index: 2 },
            ]
        );

        enter_browse_selection(&mut state, MenuSelection::Back);
        assert_eq!(state.browse_path, effect_path);
    }

    #[test]
    fn non_empty_query_flattens_matches_in_catalog_order() {
        let items = fixture();
        let filtered = filter_searchable_items(&items, "e");
        assert_eq!(filtered, vec![0, 1, 2, 3]);
        assert_eq!(
            menu_rows(
                &items,
                &filtered,
                "e",
                &["Effect".to_owned(), "Image".to_owned()],
            ),
            vec![
                MenuRow::Item { index: 0 },
                MenuRow::Item { index: 1 },
                MenuRow::Item { index: 2 },
                MenuRow::Item { index: 3 },
            ]
        );
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
            selected: Some(MenuSelection::Item(0)),
            ..MenuState::default()
        };

        state.set_selection(Some(MenuSelection::Item(1)), true);
        assert_eq!(state.take_scroll_request(), Some(MenuSelection::Item(1)));
        assert_eq!(state.take_scroll_request(), None);

        state.set_selection(Some(MenuSelection::Item(1)), true);
        assert_eq!(state.take_scroll_request(), None);

        state.set_selection(Some(MenuSelection::Item(0)), false);
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
        assert_eq!(placement.position, Pos2::new(492.0, 260.0));
        let size = Vec2::new(placement.width, placement.max_height);
        assert_eq!(size, Vec2::new(300.0, 320.0));
        assert!(viewport.contains(placement.position));
        assert!(viewport.contains(placement.position + size));
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
        assert_eq!(placement.position.y, 8.0);
        assert_eq!(placement.max_height, 492.0);
    }
}
