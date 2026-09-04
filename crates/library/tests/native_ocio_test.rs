use anyhow::{Context, Result, ensure};
use library::editor::color_service::ColorSpaceManager;
use std::collections::HashSet;

fn validate_color_space_names(spaces: &[String]) -> Result<()> {
    let mut unique = HashSet::new();
    for space in spaces {
        ensure!(
            !space.trim().is_empty(),
            "OCIO returned an empty color-space name"
        );
        ensure!(
            unique.insert(space),
            "OCIO returned duplicate color-space name {space:?}"
        );
    }
    Ok(())
}

#[test]
fn native_ocio_availability_contract_is_stable() -> Result<()> {
    let spaces = ColorSpaceManager::get_available_colorspaces();
    let repeated = ColorSpaceManager::get_available_colorspaces();
    ensure!(
        spaces == repeated,
        "OCIO color-space enumeration changed while using the same global context"
    );
    validate_color_space_names(&spaces)?;

    if spaces.is_empty() {
        // The native shim and its configured color spaces are optional. An
        // empty list is the explicit unavailable state, not an integration
        // success; the ignored test below owns the executable contract.
        return Ok(());
    }

    Ok(())
}

#[test]
#[ignore = "requires OCIO shim and configured color spaces"]
fn native_ocio_same_space_processor_is_byte_exact() -> Result<()> {
    let spaces = ColorSpaceManager::get_available_colorspaces();
    ensure!(
        !spaces.is_empty(),
        "native OCIO integration returned no color spaces; shim/context/config is unavailable"
    );
    validate_color_space_names(&spaces)?;

    let space = spaces
        .first()
        .context("non-empty OCIO color-space list lost its first element")?;
    let processor = ColorSpaceManager::create_processor(space, space).with_context(|| {
        format!("failed to create identity OCIO processor for color space {space:?}")
    })?;
    let pixels = [0_u8, 64, 128, 255, 255, 128, 64, 0];
    let transformed = processor.apply_rgba(&pixels);
    ensure!(
        transformed == pixels,
        "same-space OCIO processor for {space:?} was not byte-exact identity: input={pixels:?}, output={transformed:?}"
    );

    Ok(())
}
