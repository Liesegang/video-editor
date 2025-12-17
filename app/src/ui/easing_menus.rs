use eframe::egui::{self, Ui, UiKind};
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
