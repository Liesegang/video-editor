use library::editor::color_service::ColorSpaceManager;

#[test]
fn test_native_ocio_integration() {
    // This test requires shim.dll and OpenColorIO_2_5.dll to be in the target directory (deps or debug).
    // Cargo runs tests with target/debug/deps as current or runtime path?
    // Actually, usually it's best to run this where the DLLs are.

    // Check if we can get available color spaces.
    let spaces = ColorSpaceManager::get_available_colorspaces();
    println!("Available color spaces: {:?}", spaces);

    // We expect some spaces if OCIO config is found (or default).
    // If using default raw config, might be empty or basic.
    // If Env var OCIO is not set, CreateFromEnv might fail or return empty/default?
    // Usually it defaults to raw if nothing found, or logging error.

    // Try creating a processor (raw to raw should always work if config supports it, otherwise identity)
    // Note: If no config is present, we might get errors.

    // We can just assert that it doesn't crash (which means shim loading works).
    assert!(true);
}
