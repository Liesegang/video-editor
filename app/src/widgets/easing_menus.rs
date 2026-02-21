use eframe::egui::Ui;
use library::animation::EasingFunction;

pub fn show_easing_menu(
    ui: &mut Ui,
    current_easing: Option<&EasingFunction>,
    mut on_select: impl FnMut(EasingFunction),
) {
    let mut item = |ui: &mut Ui, label: &str, easing: EasingFunction| {
        let selected = current_easing.map_or(false, |c| {
            std::mem::discriminant(c) == std::mem::discriminant(&easing)
        });
        // Use selectable_label for highlighting if selected, but regular button behavior mostly
        if ui.selectable_label(selected, label).clicked() {
            on_select(easing);
            // Caller handles closing menu if needed
        }
    };

    item(ui, "Linear", EasingFunction::Linear);
    item(ui, "Constant", EasingFunction::Constant);

    ui.separator();

    ui.menu_button("Sine", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInSine);
        item(ui, "Ease Out", EasingFunction::EaseOutSine);
        item(ui, "Ease In Out", EasingFunction::EaseInOutSine);
    });

    ui.menu_button("Quad", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInQuad);
        item(ui, "Ease Out", EasingFunction::EaseOutQuad);
        item(ui, "Ease In Out", EasingFunction::EaseInOutQuad);
    });

    ui.menu_button("Cubic", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInCubic);
        item(ui, "Ease Out", EasingFunction::EaseOutCubic);
        item(ui, "Ease In Out", EasingFunction::EaseInOutCubic);
    });

    ui.menu_button("Quart", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInQuart);
        item(ui, "Ease Out", EasingFunction::EaseOutQuart);
        item(ui, "Ease In Out", EasingFunction::EaseInOutQuart);
    });

    ui.menu_button("Quint", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInQuint);
        item(ui, "Ease Out", EasingFunction::EaseOutQuint);
        item(ui, "Ease In Out", EasingFunction::EaseInOutQuint);
    });

    ui.menu_button("Expo", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInExpo);
        item(ui, "Ease Out", EasingFunction::EaseOutExpo);
        item(ui, "Ease In Out", EasingFunction::EaseInOutExpo);
    });

    ui.menu_button("Circ", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInCirc);
        item(ui, "Ease Out", EasingFunction::EaseOutCirc);
        item(ui, "Ease In Out", EasingFunction::EaseInOutCirc);
    });

    ui.menu_button("Back", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInBack { c1: 1.70158 });
        item(ui, "Ease Out", EasingFunction::EaseOutBack { c1: 1.70158 });
        item(
            ui,
            "Ease In Out",
            EasingFunction::EaseInOutBack { c1: 1.70158 },
        );
    });

    ui.menu_button("Elastic", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInElastic { period: 3.0 });
        item(
            ui,
            "Ease Out",
            EasingFunction::EaseOutElastic { period: 3.0 },
        );
        item(
            ui,
            "Ease In Out",
            EasingFunction::EaseInOutElastic { period: 4.5 },
        );
    });

    ui.menu_button("Bounce", |ui| {
        item(
            ui,
            "Ease In",
            EasingFunction::EaseInBounce {
                n1: 7.5625,
                d1: 2.75,
            },
        );
        item(
            ui,
            "Ease Out",
            EasingFunction::EaseOutBounce {
                n1: 7.5625,
                d1: 2.75,
            },
        );
        item(
            ui,
            "Ease In Out",
            EasingFunction::EaseInOutBounce {
                n1: 7.5625,
                d1: 2.75,
            },
        );
    });

    ui.menu_button("Custom", |ui| {
        item(
            ui,
            "Expression",
            EasingFunction::Expression {
                text: "t".to_string(),
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    // ── Domain: Top-Level Easing Items ──

    #[test]
    fn shows_linear_and_constant() {
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 600.0))
            .build_ui(|ui| {
                show_easing_menu(ui, None, |_| {});
            });
        assert!(harness.query_by_label("Linear").is_some());
        assert!(harness.query_by_label("Constant").is_some());
    }

    // ── Domain: Category Buttons ──

    #[test]
    fn shows_all_easing_category_buttons() {
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 600.0))
            .build_ui(|ui| {
                show_easing_menu(ui, None, |_| {});
            });
        assert!(harness.query_by_label("Sine").is_some());
        assert!(harness.query_by_label("Quad").is_some());
        assert!(harness.query_by_label("Cubic").is_some());
        assert!(harness.query_by_label("Quart").is_some());
        assert!(harness.query_by_label("Quint").is_some());
        assert!(harness.query_by_label("Expo").is_some());
        assert!(harness.query_by_label("Circ").is_some());
        assert!(harness.query_by_label("Back").is_some());
        assert!(harness.query_by_label("Elastic").is_some());
        assert!(harness.query_by_label("Bounce").is_some());
        assert!(harness.query_by_label("Custom").is_some());
    }

    // ── Domain: Current Selection Highlighting ──

    #[test]
    fn renders_with_current_easing_selected() {
        let current = EasingFunction::Linear;
        let harness = Harness::builder()
            .with_size(egui::vec2(200.0, 600.0))
            .build_ui(move |ui| {
                show_easing_menu(ui, Some(&current), |_| {});
            });
        // Should render without panic; Linear is highlighted via selectable_label
        assert!(harness.query_by_label("Linear").is_some());
    }

    // ── Domain: Interaction (Dynamic / Selenium-style) ──

    #[test]
    fn click_linear_triggers_callback() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let selected: Rc<RefCell<Vec<EasingFunction>>> = Rc::new(RefCell::new(Vec::new()));
        let s = selected.clone();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 600.0))
            .build_ui(move |ui| {
                let s2 = s.clone();
                show_easing_menu(ui, None, move |easing| {
                    s2.borrow_mut().push(easing);
                });
            });

        harness.get_by_label("Linear").click();
        harness.run();

        let results = selected.borrow();
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], EasingFunction::Linear));
    }

    #[test]
    fn click_constant_triggers_callback() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let selected: Rc<RefCell<Vec<EasingFunction>>> = Rc::new(RefCell::new(Vec::new()));
        let s = selected.clone();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 600.0))
            .build_ui(move |ui| {
                let s2 = s.clone();
                show_easing_menu(ui, None, move |easing| {
                    s2.borrow_mut().push(easing);
                });
            });

        harness.get_by_label("Constant").click();
        harness.run();

        let results = selected.borrow();
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], EasingFunction::Constant));
    }

    #[test]
    fn reselecting_current_easing_still_triggers_callback() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let selected: Rc<RefCell<Vec<EasingFunction>>> = Rc::new(RefCell::new(Vec::new()));
        let s = selected.clone();
        let current = EasingFunction::Linear;

        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 600.0))
            .build_ui(move |ui| {
                let s2 = s.clone();
                show_easing_menu(ui, Some(&current), move |easing| {
                    s2.borrow_mut().push(easing);
                });
            });

        // Even though Linear is already selected, clicking it should still fire callback
        harness.get_by_label("Linear").click();
        harness.run();

        let results = selected.borrow();
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], EasingFunction::Linear));
    }
}
