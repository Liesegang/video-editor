use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
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
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    // Elastic
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,
    // Bounce
    EaseInBounce,
    EaseOutBounce,
    EaseInOutBounce,
    // Custom
    SimpleBezier {
        start: (f64, f64),
        end: (f64, f64),
    },
    Bezier {
        points: Vec<(f64, f64)>,
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
            EasingFunction::EaseInBack => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                c3 * t * t * t - c1 * t * t
            }
            EasingFunction::EaseOutBack => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
            EasingFunction::EaseInOutBack => {
                let c1 = 1.70158;
                let c2 = c1 * 1.525;
                if t < 0.5 {
                    ((2.0 * t).powi(2) * ((c2 + 1.0) * 2.0 * t - c2)) / 2.0
                } else {
                    ((2.0 * t - 2.0).powi(2) * ((c2 + 1.0) * (t * 2.0 - 2.0) + c2) + 2.0) / 2.0
                }
            }
            EasingFunction::EaseInElastic => {
                let c4 = (2.0 * std::f64::consts::PI) / 3.0;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    -2.0_f64.powf(10.0 * t - 10.0) * ((t * 10.0 - 10.75) * c4).sin()
                }
            }
            EasingFunction::EaseOutElastic => {
                let c4 = (2.0 * std::f64::consts::PI) / 3.0;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    2.0_f64.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
                }
            }
            EasingFunction::EaseInOutElastic => {
                let c5 = (2.0 * std::f64::consts::PI) / 4.5;
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
            EasingFunction::EaseInBounce => 1.0 - Self::bounce_out(1.0 - t),
            EasingFunction::EaseOutBounce => Self::bounce_out(t),
            EasingFunction::EaseInOutBounce => {
                if t < 0.5 {
                    (1.0 - Self::bounce_out(1.0 - 2.0 * t)) / 2.0
                } else {
                    (1.0 + Self::bounce_out(2.0 * t - 1.0)) / 2.0
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
        }
    }

    fn bounce_out(t: f64) -> f64 {
        let n1 = 7.5625;
        let d1 = 2.75;

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
