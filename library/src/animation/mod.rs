use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde::{Deserialize, Serialize};

use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Clone, Debug, Default)] // Removed PartialEq, Eq, Hash, Copy; Added Default
pub enum EasingFunction {
    #[default]
    Linear,
    // Sine
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
    // Quad
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    // Cubic
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
    // Quart
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,
    // Quint
    EaseInQuint,
    EaseOutQuint,
    EaseInOutQuint,
    // Expo
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,
    // Circ
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,
    // Back
    EaseInBack {
        c1: f64,
    },
    EaseOutBack {
        c1: f64,
    },
    EaseInOutBack {
        c1: f64,
    },
    // Elastic
    EaseInElastic {
        period: f64,
    },
    EaseOutElastic {
        period: f64,
    },
    EaseInOutElastic {
        period: f64,
    },
    // Bounce
    EaseInBounce {
        n1: f64,
        d1: f64,
    },
    EaseOutBounce {
        n1: f64,
        d1: f64,
    },
    EaseInOutBounce {
        n1: f64,
        d1: f64,
    },
    // Custom
    SimpleBezier {
        start: (f64, f64),
        end: (f64, f64),
    },
    Bezier {
        points: Vec<(f64, f64)>,
    },
    #[serde(rename = "Expression")]
    Expression {
        text: String,
    },
}

impl EasingFunction {
    pub fn apply(&self, t: f64) -> f64 {
        match self {
            EasingFunction::Linear => t,
            EasingFunction::EaseInSine => 1.0 - (t * std::f64::consts::PI / 2.0).cos(),
            EasingFunction::EaseOutSine => (t * std::f64::consts::PI / 2.0).sin(),
            EasingFunction::EaseInOutSine => -(std::f64::consts::PI * t).cos() / 2.0 + 0.5,
            EasingFunction::EaseInQuad => t * t,
            EasingFunction::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            EasingFunction::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            EasingFunction::EaseInCubic => t * t * t,
            EasingFunction::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            EasingFunction::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            EasingFunction::EaseInQuart => t * t * t * t,
            EasingFunction::EaseOutQuart => 1.0 - (1.0 - t).powi(4),
            EasingFunction::EaseInOutQuart => {
                if t < 0.5 {
                    8.0 * t * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(4) / 2.0
                }
            }
            EasingFunction::EaseInQuint => t * t * t * t * t,
            EasingFunction::EaseOutQuint => 1.0 - (1.0 - t).powi(5),
            EasingFunction::EaseInOutQuint => {
                if t < 0.5 {
                    16.0 * t * t * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(5) / 2.0
                }
            }
            EasingFunction::EaseInExpo => {
                if t == 0.0 {
                    0.0
                } else {
                    2.0_f64.powf(10.0 * t - 10.0)
                }
            }
            EasingFunction::EaseOutExpo => {
                if t == 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f64.powf(-10.0 * t)
                }
            }
            EasingFunction::EaseInOutExpo => {
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else if t < 0.5 {
                    2.0_f64.powf(20.0 * t - 10.0) / 2.0
                } else {
                    (2.0 - 2.0_f64.powf(-20.0 * t + 10.0)) / 2.0
                }
            }
            EasingFunction::EaseInCirc => 1.0 - (1.0 - t * t).sqrt(),
            EasingFunction::EaseOutCirc => (1.0 - (t - 1.0).powi(2)).sqrt(),
            EasingFunction::EaseInOutCirc => {
                if t < 0.5 {
                    (1.0 - (1.0 - (2.0 * t).powi(2)).sqrt()) / 2.0
                } else {
                    ((1.0 - (-2.0 * t + 2.0).powi(2)).sqrt() + 1.0) / 2.0
                }
            }
            EasingFunction::EaseInBack { c1 } => {
                let c3 = c1 + 1.0;
                c3 * t * t * t - c1 * t * t
            }
            EasingFunction::EaseOutBack { c1 } => {
                let c3 = c1 + 1.0;
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
            EasingFunction::EaseInOutBack { c1 } => {
                let c2 = c1 * 1.525;
                if t < 0.5 {
                    ((2.0 * t).powi(2) * ((c2 + 1.0) * 2.0 * t - c2)) / 2.0
                } else {
                    ((2.0 * t - 2.0).powi(2) * ((c2 + 1.0) * (t * 2.0 - 2.0) + c2) + 2.0) / 2.0
                }
            }
            EasingFunction::EaseInElastic { period } => {
                let c4 = (2.0 * std::f64::consts::PI) / period;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    -2.0_f64.powf(10.0 * t - 10.0) * ((t * 10.0 - 10.75) * c4).sin()
                }
            }
            EasingFunction::EaseOutElastic { period } => {
                let c4 = (2.0 * std::f64::consts::PI) / period;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    2.0_f64.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
                }
            }
            EasingFunction::EaseInOutElastic { period } => {
                let c5 = (2.0 * std::f64::consts::PI) / period;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else if t < 0.5 {
                    -(2.0_f64.powf(20.0 * t - 10.0) * ((20.0 * t - 11.125) * c5).sin()) / 2.0
                } else {
                    (2.0_f64.powf(-20.0 * t + 10.0) * ((20.0 * t - 11.125) * c5).sin()) / 2.0 + 1.0
                }
            }
            EasingFunction::EaseInBounce { n1, d1 } => 1.0 - Self::bounce_out(1.0 - t, *n1, *d1),
            EasingFunction::EaseOutBounce { n1, d1 } => Self::bounce_out(t, *n1, *d1),
            EasingFunction::EaseInOutBounce { n1, d1 } => {
                if t < 0.5 {
                    (1.0 - Self::bounce_out(1.0 - 2.0 * t, *n1, *d1)) / 2.0
                } else {
                    (1.0 + Self::bounce_out(2.0 * t - 1.0, *n1, *d1)) / 2.0
                }
            }
            EasingFunction::SimpleBezier { start, end } => {
                let max_iterations = 16;
                let epsilon = 1e-6;
                let mut current_t = t;

                for _ in 0..max_iterations {
                    let one_minus_t = 1.0 - current_t;
                    let one_minus_t_squared = one_minus_t * one_minus_t;
                    let t_squared = current_t * current_t;
                    let t_cubed = t_squared * current_t;

                    let y = 3.0 * one_minus_t_squared * current_t * start.1
                        + 3.0 * one_minus_t * t_squared * end.1
                        + t_cubed;

                    if (y - t).abs() < epsilon {
                        break;
                    }

                    let dy_dt = 3.0 * one_minus_t_squared * start.1
                        + 6.0 * one_minus_t * current_t * (end.1 - start.1)
                        + 3.0 * t_squared * (1.0 - end.1);

                    if dy_dt.abs() < epsilon {
                        break;
                    }

                    current_t = current_t - (y - t) / dy_dt;

                    current_t = current_t.max(0.0).min(1.0);
                }

                let one_minus_t = 1.0 - current_t;
                let one_minus_t_squared = one_minus_t * one_minus_t;
                let t_squared = current_t * current_t;
                let t_cubed = t_squared * current_t;

                3.0 * one_minus_t_squared * current_t * start.0
                    + 3.0 * one_minus_t * t_squared * end.0
                    + t_cubed
            }
            EasingFunction::Bezier { points } => {
                if points.is_empty() {
                    return t;
                }

                let mut all_points = Vec::with_capacity(points.len() + 2);
                all_points.push((0.0, 0.0));
                all_points.extend_from_slice(points);
                all_points.push((1.0, 1.0));

                let max_iterations = 16;
                let epsilon = 1e-6;
                let mut current_t = t;

                for _ in 0..max_iterations {
                    let (_, y) = EasingFunction::evaluate_bezier(&all_points, current_t);

                    if (y - t).abs() < epsilon {
                        break;
                    }

                    let delta = 0.001;
                    let (_, y_plus) =
                        EasingFunction::evaluate_bezier(&all_points, current_t + delta);
                    let dy_dt = (y_plus - y) / delta;

                    if dy_dt.abs() < epsilon {
                        break;
                    }

                    current_t = current_t - (y - t) / dy_dt;

                    current_t = current_t.max(0.0).min(1.0);
                }

                let (x, _) = EasingFunction::evaluate_bezier(&all_points, current_t);
                x
            }
            Self::Expression { text } => Python::attach(|py| {
                let locals = PyDict::new(py);
                if let Err(e) = locals.set_item("t", t) {
                    log::error!("Failed to set 't' in python context: {}", e);
                    return t;
                }

                let builtins = match PyModule::import(py, "builtins") {
                    Ok(m) => m,
                    Err(e) => {
                        log::error!("Failed to import builtins: {}", e);
                        return t;
                    }
                };

                let eval_func = match builtins.getattr("eval") {
                    Ok(f) => f,
                    Err(e) => {
                        log::error!("Failed to get eval: {}", e);
                        return t;
                    }
                };

                let globals = PyDict::new(py);

                if let Ok(math_mod) = PyModule::import(py, "math") {
                    let _ = globals.set_item("math", math_mod);
                } else {
                    log::warn!("Failed to import math module for expression");
                }

                if let Ok(random_mod) = PyModule::import(py, "random") {
                    let _ = globals.set_item("random", random_mod);
                } else {
                    log::warn!("Failed to import random module for expression");
                }

                match eval_func.call1((text.as_str(), Some(&globals), Some(&locals))) {
                    Ok(result) => match result.extract::<f64>() {
                        Ok(val) => val,
                        Err(e) => {
                            log::error!("Expression result is not a float: {}", e);
                            t
                        }
                    },
                    Err(e) => {
                        log::error!("Failed to evaluate expression: {}", e);
                        t
                    }
                }
            }),
        }
    }

    fn bounce_out(t: f64, n1: f64, d1: f64) -> f64 {
        if t < 1.0 / d1 {
            n1 * t * t
        } else if t < 2.0 / d1 {
            let t = t - 1.5 / d1;
            n1 * t * t + 0.75
        } else if t < 2.5 / d1 {
            let t = t - 2.25 / d1;
            n1 * t * t + 0.9375
        } else {
            let t = t - 2.625 / d1;
            n1 * t * t + 0.984375
        }
    }

    fn evaluate_bezier(points: &[(f64, f64)], t: f64) -> (f64, f64) {
        if points.len() == 1 {
            return points[0];
        }

        let mut temp = Vec::with_capacity(points.len() - 1);
        for i in 0..points.len() - 1 {
            let x = (1.0 - t) * points[i].0 + t * points[i + 1].0;
            let y = (1.0 - t) * points[i].1 + t * points[i + 1].1;
            temp.push((x, y));
        }

        Self::evaluate_bezier(&temp, t)
    }
}

impl PartialEq for EasingFunction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (EasingFunction::Linear, EasingFunction::Linear) => true,
            (EasingFunction::EaseInSine, EasingFunction::EaseInSine) => true,
            (EasingFunction::EaseOutSine, EasingFunction::EaseOutSine) => true,
            (EasingFunction::EaseInOutSine, EasingFunction::EaseInOutSine) => true,
            (EasingFunction::EaseInQuad, EasingFunction::EaseInQuad) => true,
            (EasingFunction::EaseOutQuad, EasingFunction::EaseOutQuad) => true,
            (EasingFunction::EaseInOutQuad, EasingFunction::EaseInOutQuad) => true,
            (EasingFunction::EaseInCubic, EasingFunction::EaseInCubic) => true,
            (EasingFunction::EaseOutCubic, EasingFunction::EaseOutCubic) => true,
            (EasingFunction::EaseInOutCubic, EasingFunction::EaseInOutCubic) => true,
            (EasingFunction::EaseInQuart, EasingFunction::EaseInQuart) => true,
            (EasingFunction::EaseOutQuart, EasingFunction::EaseOutQuart) => true,
            (EasingFunction::EaseInOutQuart, EasingFunction::EaseInOutQuart) => true,
            (EasingFunction::EaseInQuint, EasingFunction::EaseInQuint) => true,
            (EasingFunction::EaseOutQuint, EasingFunction::EaseOutQuint) => true,
            (EasingFunction::EaseInOutQuint, EasingFunction::EaseInOutQuint) => true,
            (EasingFunction::EaseInExpo, EasingFunction::EaseInExpo) => true,
            (EasingFunction::EaseOutExpo, EasingFunction::EaseOutExpo) => true,
            (EasingFunction::EaseInOutExpo, EasingFunction::EaseInOutExpo) => true,
            (EasingFunction::EaseInCirc, EasingFunction::EaseInCirc) => true,
            (EasingFunction::EaseOutCirc, EasingFunction::EaseOutCirc) => true,
            (EasingFunction::EaseInOutCirc, EasingFunction::EaseInOutCirc) => true,
            (EasingFunction::EaseInBack { c1: a }, EasingFunction::EaseInBack { c1: b }) => {
                OrderedFloat(*a) == OrderedFloat(*b)
            }
            (EasingFunction::EaseOutBack { c1: a }, EasingFunction::EaseOutBack { c1: b }) => {
                OrderedFloat(*a) == OrderedFloat(*b)
            }
            (EasingFunction::EaseInOutBack { c1: a }, EasingFunction::EaseInOutBack { c1: b }) => {
                OrderedFloat(*a) == OrderedFloat(*b)
            }
            (
                EasingFunction::EaseInElastic { period: a },
                EasingFunction::EaseInElastic { period: b },
            ) => OrderedFloat(*a) == OrderedFloat(*b),
            (
                EasingFunction::EaseOutElastic { period: a },
                EasingFunction::EaseOutElastic { period: b },
            ) => OrderedFloat(*a) == OrderedFloat(*b),
            (
                EasingFunction::EaseInOutElastic { period: a },
                EasingFunction::EaseInOutElastic { period: b },
            ) => OrderedFloat(*a) == OrderedFloat(*b),
            (
                EasingFunction::EaseInBounce { n1: a, d1: b },
                EasingFunction::EaseInBounce { n1: c, d1: d },
            ) => OrderedFloat(*a) == OrderedFloat(*c) && OrderedFloat(*b) == OrderedFloat(*d),
            (
                EasingFunction::EaseOutBounce { n1: a, d1: b },
                EasingFunction::EaseOutBounce { n1: c, d1: d },
            ) => OrderedFloat(*a) == OrderedFloat(*c) && OrderedFloat(*b) == OrderedFloat(*d),
            (
                EasingFunction::EaseInOutBounce { n1: a, d1: b },
                EasingFunction::EaseInOutBounce { n1: c, d1: d },
            ) => OrderedFloat(*a) == OrderedFloat(*c) && OrderedFloat(*b) == OrderedFloat(*d),
            (
                EasingFunction::SimpleBezier { start: s1, end: e1 },
                EasingFunction::SimpleBezier { start: s2, end: e2 },
            ) => {
                OrderedFloat(s1.0) == OrderedFloat(s2.0)
                    && OrderedFloat(s1.1) == OrderedFloat(s2.1)
                    && OrderedFloat(e1.0) == OrderedFloat(e2.0)
                    && OrderedFloat(e1.1) == OrderedFloat(e2.1)
            }
            (EasingFunction::Bezier { points: p1 }, EasingFunction::Bezier { points: p2 }) => {
                p1.len() == p2.len()
                    && p1.iter().zip(p2.iter()).all(|(a, b)| {
                        OrderedFloat(a.0) == OrderedFloat(b.0)
                            && OrderedFloat(a.1) == OrderedFloat(b.1)
                    })
            }
            (EasingFunction::Expression { text: t1 }, EasingFunction::Expression { text: t2 }) => {
                t1 == t2
            }
            _ => false,
        }
    }
}

impl Eq for EasingFunction {}

impl Hash for EasingFunction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            EasingFunction::EaseInBack { c1 }
            | EasingFunction::EaseOutBack { c1 }
            | EasingFunction::EaseInOutBack { c1 } => {
                OrderedFloat(*c1).hash(state);
            }
            EasingFunction::EaseInElastic { period }
            | EasingFunction::EaseOutElastic { period }
            | EasingFunction::EaseInOutElastic { period } => {
                OrderedFloat(*period).hash(state);
            }
            EasingFunction::EaseInBounce { n1, d1 }
            | EasingFunction::EaseOutBounce { n1, d1 }
            | EasingFunction::EaseInOutBounce { n1, d1 } => {
                OrderedFloat(*n1).hash(state);
                OrderedFloat(*d1).hash(state);
            }
            EasingFunction::SimpleBezier { start, end } => {
                OrderedFloat(start.0).hash(state);
                OrderedFloat(start.1).hash(state);
                OrderedFloat(end.0).hash(state);
                OrderedFloat(end.1).hash(state);
            }
            EasingFunction::Bezier { points } => {
                for p in points {
                    OrderedFloat(p.0).hash(state);
                    OrderedFloat(p.1).hash(state);
                }
            }
            EasingFunction::Expression { text } => {
                text.hash(state);
            }
            _ => {} // Unit variants only hash discriminant
        }
    }
}
