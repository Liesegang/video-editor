use eframe::egui::{self};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MyNodeTemplate {
    MakeScalar,
    AddScalar,
    SubtractScalar,
    MultiplyScalar,
    MakeVector,
    AddVector,
    Print,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MyValueType {
    Scalar(f32),
    Vector(egui::Vec2),
}
