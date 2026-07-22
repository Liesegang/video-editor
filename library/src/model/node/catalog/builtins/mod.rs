//! Implemented first-party nodes, grouped by runtime responsibility.

use super::descriptor::DescriptorSpec;

mod compositing;
mod data;
mod generators;
mod list;
mod math;
mod sound;

pub(super) fn specs() -> impl Iterator<Item = &'static DescriptorSpec> {
    generators::specs()
        .iter()
        .chain(data::specs())
        .chain(math::specs())
        .chain(list::specs())
        .chain(compositing::specs())
        .chain(sound::specs())
}
