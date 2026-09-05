use eframe::egui::{DragValue, TextEdit, Ui};
use library::animation::EasingFunction;

#[derive(Clone, Copy)]
pub struct EasingMenuQaScope<'a> {
    id_prefix: &'a str,
    id_suffix: &'a str,
}

impl<'a> EasingMenuQaScope<'a> {
    pub const fn new(id_prefix: &'a str, id_suffix: &'a str) -> Self {
        Self {
            id_prefix,
            id_suffix,
        }
    }

    fn component_id(self, option: &str) -> String {
        format!("{}.{}:{}", self.id_prefix, option, self.id_suffix)
    }
}

fn qa_option(easing: &EasingFunction) -> Option<&'static str> {
    match easing {
        EasingFunction::Linear => Some("linear"),
        EasingFunction::Constant => Some("constant"),
        EasingFunction::EaseInCubic => Some("ease_in_cubic"),
        EasingFunction::EaseOutCubic => Some("ease_out_cubic"),
        EasingFunction::EaseInOutCubic => Some("ease_in_out_cubic"),
        _ => None,
    }
}

/// Compact label shared by controls that summarize the current interpolation.
pub fn easing_summary(easing: &EasingFunction) -> &'static str {
    match easing {
        EasingFunction::Linear => "Linear",
        EasingFunction::Constant => "Hold",
        EasingFunction::EaseInSine
        | EasingFunction::EaseInQuad
        | EasingFunction::EaseInCubic
        | EasingFunction::EaseInQuart
        | EasingFunction::EaseInQuint
        | EasingFunction::EaseInExpo
        | EasingFunction::EaseInCirc
        | EasingFunction::EaseInBack { .. }
        | EasingFunction::EaseInElastic { .. }
        | EasingFunction::EaseInBounce { .. } => "Ease In",
        EasingFunction::EaseOutSine
        | EasingFunction::EaseOutQuad
        | EasingFunction::EaseOutCubic
        | EasingFunction::EaseOutQuart
        | EasingFunction::EaseOutQuint
        | EasingFunction::EaseOutExpo
        | EasingFunction::EaseOutCirc
        | EasingFunction::EaseOutBack { .. }
        | EasingFunction::EaseOutElastic { .. }
        | EasingFunction::EaseOutBounce { .. } => "Ease Out",
        EasingFunction::EaseInOutSine
        | EasingFunction::EaseInOutQuad
        | EasingFunction::EaseInOutCubic
        | EasingFunction::EaseInOutQuart
        | EasingFunction::EaseInOutQuint
        | EasingFunction::EaseInOutExpo
        | EasingFunction::EaseInOutCirc
        | EasingFunction::EaseInOutBack { .. }
        | EasingFunction::EaseInOutElastic { .. }
        | EasingFunction::EaseInOutBounce { .. } => "Ease In / Out",
        EasingFunction::SimpleBezier { .. } | EasingFunction::Bezier { .. } => "Custom Bezier",
        EasingFunction::Expression { .. } => "Expression",
    }
}

pub fn show_easing_menu(
    ui: &mut Ui,
    current_easing: Option<&EasingFunction>,
    qa_scope: Option<EasingMenuQaScope<'_>>,
    mut on_select: impl FnMut(EasingFunction),
) {
    let mut item = |ui: &mut Ui, label: &str, easing: EasingFunction| {
        let selected = current_easing
            .is_some_and(|c| std::mem::discriminant(c) == std::mem::discriminant(&easing));
        let response = ui.selectable_label(selected, label);
        if let (Some(scope), Some(option)) = (qa_scope, qa_option(&easing)) {
            crate::qa::register_component_with_metadata(
                scope.component_id(option),
                "easing_menu_option",
                response.rect,
                response.enabled(),
                Some(serde_json::json!({
                    "option": option,
                    "label": label,
                    "selected": selected,
                })),
            );
        }
        if response.clicked() {
            on_select(easing);
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

    let cubic = ui.menu_button("Cubic", |ui| {
        item(ui, "Ease In", EasingFunction::EaseInCubic);
        item(ui, "Ease Out", EasingFunction::EaseOutCubic);
        item(ui, "Ease In Out", EasingFunction::EaseInOutCubic);
    });
    if let Some(scope) = qa_scope {
        crate::qa::register_component_with_metadata(
            scope.component_id("family.cubic"),
            "easing_menu_family",
            cubic.response.rect,
            cubic.response.enabled(),
            Some(serde_json::json!({
                "family": "cubic",
                "label": "Cubic",
            })),
        );
    }

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

/// Exact variant label for edit surfaces where the selected family matters.
pub fn easing_name(easing: &EasingFunction) -> &'static str {
    match easing {
        EasingFunction::Linear => "Linear",
        EasingFunction::Constant => "Constant",
        EasingFunction::EaseInSine => "Ease In Sine",
        EasingFunction::EaseOutSine => "Ease Out Sine",
        EasingFunction::EaseInOutSine => "Ease In Out Sine",
        EasingFunction::EaseInQuad => "Ease In Quad",
        EasingFunction::EaseOutQuad => "Ease Out Quad",
        EasingFunction::EaseInOutQuad => "Ease In Out Quad",
        EasingFunction::EaseInCubic => "Ease In Cubic",
        EasingFunction::EaseOutCubic => "Ease Out Cubic",
        EasingFunction::EaseInOutCubic => "Ease In Out Cubic",
        EasingFunction::EaseInQuart => "Ease In Quart",
        EasingFunction::EaseOutQuart => "Ease Out Quart",
        EasingFunction::EaseInOutQuart => "Ease In Out Quart",
        EasingFunction::EaseInQuint => "Ease In Quint",
        EasingFunction::EaseOutQuint => "Ease Out Quint",
        EasingFunction::EaseInOutQuint => "Ease In Out Quint",
        EasingFunction::EaseInExpo => "Ease In Expo",
        EasingFunction::EaseOutExpo => "Ease Out Expo",
        EasingFunction::EaseInOutExpo => "Ease In Out Expo",
        EasingFunction::EaseInCirc => "Ease In Circ",
        EasingFunction::EaseOutCirc => "Ease Out Circ",
        EasingFunction::EaseInOutCirc => "Ease In Out Circ",
        EasingFunction::EaseInBack { .. } => "Ease In Back",
        EasingFunction::EaseOutBack { .. } => "Ease Out Back",
        EasingFunction::EaseInOutBack { .. } => "Ease In Out Back",
        EasingFunction::EaseInElastic { .. } => "Ease In Elastic",
        EasingFunction::EaseOutElastic { .. } => "Ease Out Elastic",
        EasingFunction::EaseInOutElastic { .. } => "Ease In Out Elastic",
        EasingFunction::EaseInBounce { .. } => "Ease In Bounce",
        EasingFunction::EaseOutBounce { .. } => "Ease Out Bounce",
        EasingFunction::EaseInOutBounce { .. } => "Ease In Out Bounce",
        EasingFunction::SimpleBezier { .. } | EasingFunction::Bezier { .. } => "Custom Bezier",
        EasingFunction::Expression { .. } => "Expression",
    }
}

/// Edit the parameters carried by nontrivial easing variants. The caller
/// owns transaction boundaries; this shared control only edits its draft.
pub fn show_easing_parameters(ui: &mut Ui, easing: &mut EasingFunction) {
    match easing {
        EasingFunction::EaseInBack { c1 }
        | EasingFunction::EaseOutBack { c1 }
        | EasingFunction::EaseInOutBack { c1 } => {
            if !c1.is_finite() {
                *c1 = 1.70158;
            }
            ui.label("Overshoot");
            ui.add(DragValue::new(c1).speed(0.01));
        }
        EasingFunction::EaseInElastic { period }
        | EasingFunction::EaseOutElastic { period }
        | EasingFunction::EaseInOutElastic { period } => {
            if !period.is_finite() || *period <= 0.0 {
                *period = 3.0;
            }
            ui.label("Period");
            ui.add(DragValue::new(period).speed(0.01).range(0.1..=100.0));
        }
        EasingFunction::EaseInBounce { n1, d1 }
        | EasingFunction::EaseOutBounce { n1, d1 }
        | EasingFunction::EaseInOutBounce { n1, d1 } => {
            if !n1.is_finite() || *n1 <= 0.0 {
                *n1 = 7.5625;
            }
            if !d1.is_finite() || *d1 <= 0.0 {
                *d1 = 2.75;
            }
            ui.label("Amplitude");
            ui.add(DragValue::new(n1).speed(0.01).range(0.001..=10_000.0));
            ui.label("Duration factor");
            ui.add(DragValue::new(d1).speed(0.01).range(0.001..=10_000.0));
        }
        EasingFunction::Expression { text } => {
            ui.label("Expression (t is 0.0 to 1.0)");
            ui.add(TextEdit::multiline(text).code_editor().desired_rows(3));
        }
        _ => {}
    }
}
