use anyhow::{Context, Result, ensure};
use library::editor::color_service::ColorSpaceManager;
use std::collections::HashSet;

#[test]
fn test_native_ocio_integration() -> Result<()> {
    let spaces = ColorSpaceManager::get_available_colorspaces();
    let repeated = ColorSpaceManager::get_available_colorspaces();
    ensure!(
        spaces == repeated,
        "OCIO color-space enumeration changed while using the same global context"
    );

    let mut unique = HashSet::new();
    for space in &spaces {
        ensure!(
            !space.trim().is_empty(),
            "OCIO returned an empty color-space name"
        );
        ensure!(
            unique.insert(space),
            "OCIO returned duplicate color-space name {space:?}"
        );
    }

    if let Some(space) = spaces.first() {
        let processor = ColorSpaceManager::create_processor(space, space).with_context(|| {
            format!("failed to create identity OCIO processor for color space {space:?}")
        })?;
        let pixels = [0_u8, 64, 128, 255, 255, 128, 64, 0];
        let transformed = processor.apply_rgba(&pixels);
        ensure!(
            transformed.len() == pixels.len(),
            "OCIO processor changed RGBA buffer length from {} to {}",
            pixels.len(),
            transformed.len()
        );
    }

    Ok(())
}
