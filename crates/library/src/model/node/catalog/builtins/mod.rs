//! Implemented first-party nodes, grouped by runtime responsibility.

use super::descriptor::DescriptorSpec;

mod color;
mod compositing;
mod conditional;
mod data;
mod generators;
mod list;
mod math;
mod path;
mod shape_primitives;
mod sound;
mod transition;

pub(crate) use conditional::{
    CONDITION_INPUT_PORT, ConditionalNodeRole, SELECT_FALSE_INPUT_PORT, SELECT_TRUE_INPUT_PORT,
};
pub(crate) use math::{NUMERIC_LENGTH_CATALOG_ID, NUMERIC_LENGTH_INPUT_PORT};

pub(super) fn specs() -> impl Iterator<Item = &'static DescriptorSpec> {
    generators::specs()
        .iter()
        .chain(data::specs())
        .chain(color::specs())
        .chain(math::specs())
        .chain(conditional::specs())
        .chain(list::specs())
        .chain(path::specs())
        .chain(shape_primitives::specs())
        .chain(compositing::specs())
        .chain(sound::specs())
        .chain(transition::specs())
}
