use egui::Ui;

use super::types::{ContextMenu, MenuItem};

/// Render a standard (non-searchable) context menu.
/// Returns `Some(action)` if the user clicked an enabled item this frame.
pub fn show_context_menu<A: Clone + 'static>(ui: &mut Ui, menu: &ContextMenu<A>) -> Option<A> {
    let mut selected = None;
    for item in &menu.items {
        render_menu_item(ui, item, &mut selected);
    }
    selected
}

fn render_menu_item<A: Clone>(ui: &mut Ui, item: &MenuItem<A>, selected: &mut Option<A>) {
    match item {
        MenuItem::Action {
            label,
            icon,
            action,
            enabled,
            danger,
        } => {
            let text = match icon {
                Some(icon) => format!("{} {}", icon, label),
                None => label.clone(),
            };
            let rich = if *danger {
                egui::RichText::new(&text).color(egui::Color32::RED)
            } else {
                egui::RichText::new(&text)
            };
            let button = egui::Button::new(rich);
            let response = ui.add_enabled(*enabled, button);
            if response.clicked() {
                if let Some(action) = action {
                    *selected = Some(action.clone());
                    ui.close();
                }
            }
        }
        MenuItem::Separator => {
            ui.separator();
        }
        MenuItem::Label(text) => {
            ui.label(text);
        }
        MenuItem::SubMenu { label, icon, items } => {
            let text = match icon {
                Some(icon) => format!("{} {}", icon, label),
                None => label.clone(),
            };
            ui.menu_button(&text, |ui| {
                for sub_item in items {
                    render_menu_item(ui, sub_item, selected);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::context_menu::builder::ContextMenuBuilder;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    #[test]
    fn renders_action_labels() {
        let menu = ContextMenuBuilder::new()
            .action("Cut", 1)
            .action("Copy", 2)
            .action("Paste", 3)
            .build();
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(|ui| {
                show_context_menu(ui, &menu);
            });
        assert!(harness.query_by_label("Cut").is_some());
        assert!(harness.query_by_label("Copy").is_some());
        assert!(harness.query_by_label("Paste").is_some());
    }

    #[test]
    fn renders_label_items() {
        let menu = ContextMenuBuilder::new()
            .label("Keyframe 3 - position")
            .separator()
            .action("Edit", 1)
            .build();
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(|ui| {
                show_context_menu(ui, &menu);
            });
        assert!(harness.query_by_label("Keyframe 3 - position").is_some());
        assert!(harness.query_by_label("Edit").is_some());
    }

    #[test]
    fn renders_submenu_button() {
        let menu = ContextMenuBuilder::new()
            .submenu("Easing", |b| b.action("Linear", 1))
            .build();
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(|ui| {
                show_context_menu(ui, &menu);
            });
        assert!(harness.query_by_label("Easing").is_some());
    }

    #[test]
    fn clicking_action_returns_correct_value() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let menu = ContextMenuBuilder::new()
            .action("Alpha", 10)
            .action("Beta", 20)
            .build();
        let results: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let r = results.clone();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(move |ui| {
                if let Some(action) = show_context_menu(ui, &menu) {
                    r.borrow_mut().push(action);
                }
            });

        harness.get_by_label("Beta").click();
        harness.run_steps(2);

        assert_eq!(*results.borrow(), vec![20]);
    }

    #[test]
    fn empty_menu_shows_nothing() {
        let menu: ContextMenu<i32> = ContextMenuBuilder::new().build();
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(|ui| {
                show_context_menu(ui, &menu);
            });
        assert!(harness.query_by_label("Cut").is_none());
    }

    #[test]
    fn renders_action_with_icon() {
        let menu = ContextMenuBuilder::new()
            .action_with_icon("X", "Remove", 42)
            .build();
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(|ui| {
                show_context_menu(ui, &menu);
            });
        assert!(harness.query_by_label("X Remove").is_some());
    }

    #[test]
    fn renders_danger_action() {
        let menu = ContextMenuBuilder::new()
            .danger_action("!", "Delete", 1)
            .build();
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(|ui| {
                show_context_menu(ui, &menu);
            });
        assert!(harness.query_by_label("! Delete").is_some());
    }

    #[test]
    fn disabled_item_renders_but_click_returns_nothing() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let menu: ContextMenu<i32> = ContextMenuBuilder::new()
            .action("Enabled", 1)
            .disabled("Locked")
            .build();
        let results: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let r = results.clone();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 300.0))
            .build_ui(move |ui| {
                if let Some(action) = show_context_menu(ui, &menu) {
                    r.borrow_mut().push(action);
                }
            });

        // The disabled button exists but clicking should not trigger callback
        harness.run_steps(2);
        assert!(results.borrow().is_empty());
    }
}
