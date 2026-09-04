//! Implemented first-party nodes, grouped by runtime responsibility.

use super::descriptor::DescriptorSpec;

mod color;
mod compositing;
mod data;
mod generators;
mod list;
mod math;
mod path;
mod sound;

pub(super) fn specs() -> impl Iterator<Item = &'static DescriptorSpec> {
    generators::specs()
        .iter()
        .chain(data::specs())
        .chain(color::specs())
        .chain(math::specs())
        .chain(list::specs())
        .chain(path::specs())
        .chain(compositing::specs())
        .chain(sound::specs())
}
