use super::*;

static FREED_TEST_FRAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MISALIGNED_EXTENSION_POINTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static MISALIGNED_TABLE_CALLBACKS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

unsafe extern "C" fn forbidden_misaligned_descriptor_callback(
    _context: *mut std::ffi::c_void,
) -> RuvieCallResult {
    MISALIGNED_TABLE_CALLBACKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    RuvieCallResult {
        status: STATUS_OK,
        buffer: RuvieBuffer::empty(),
    }
}

unsafe extern "C" fn forbidden_misaligned_effect_create(
    _context: *mut std::ffi::c_void,
    _component_id: RuvieBytesView,
    _properties: RuviePropertyMapViewV1,
    _out_instance: *mut u64,
) -> RuvieExtensionResultV1 {
    MISALIGNED_TABLE_CALLBACKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    RuvieExtensionResultV1::error(ruvie_plugin_api::STATUS_PLUGIN_ERROR, "must not run")
}

unsafe extern "C" fn misaligned_extension_query(
    _context: *mut std::ffi::c_void,
    _name: RuvieBytesView,
) -> *const std::ffi::c_void {
    MISALIGNED_EXTENSION_POINTER.load(std::sync::atomic::Ordering::SeqCst)
        as *const std::ffi::c_void
}

#[test]
fn misaligned_base_and_extension_tables_are_rejected_before_table_use()
-> Result<(), Box<dyn std::error::Error>> {
    MISALIGNED_TABLE_CALLBACKS.store(0, std::sync::atomic::Ordering::SeqCst);
    let base = RuviePluginApiV1 {
        abi_version: RUVIE_PLUGIN_ABI_V1,
        struct_size: size_of::<RuviePluginApiV1>(),
        context: std::ptr::null_mut(),
        descriptor_json: Some(forbidden_misaligned_descriptor_callback),
        invoke_json: None,
        free_buffer: None,
        query_extension: None,
    };
    let mut base_storage =
        vec![0_u8; size_of::<RuviePluginApiV1>() + align_of::<RuviePluginApiV1>()];
    // A one-byte offset from Vec's allocation is deliberately unsuitable
    // for every ABI table whose alignment is greater than one. SAFETY: the
    // allocation includes the offset byte and a complete table.
    let base_pointer = unsafe { base_storage.as_mut_ptr().add(1) };
    // SAFETY: The backing allocation has enough space and this write is
    // explicitly unaligned; no reference is formed.
    unsafe {
        base_pointer
            .cast::<RuviePluginApiV1>()
            .write_unaligned(base)
    };
    let Err(error) = copy_abi_table::<RuviePluginApiV1>(base_pointer.cast(), "base fixture") else {
        return Err(std::io::Error::other("a misaligned entry table must not be copied").into());
    };
    let error = error.to_string();
    assert!(error.contains("misaligned ABI table"));
    assert_eq!(
        MISALIGNED_TABLE_CALLBACKS.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a callback pointer from a misaligned base table must never execute"
    );

    let extension = RuvieEffectCpuRgba8ApiV1 {
        abi_version: RUVIE_PLUGIN_ABI_V1,
        struct_size: size_of::<RuvieEffectCpuRgba8ApiV1>(),
        context: std::ptr::null_mut(),
        create_instance: Some(forbidden_misaligned_effect_create),
        process: None,
        release_instance: None,
        free_frame: None,
    };
    let mut extension_storage =
        vec![0_u8; size_of::<RuvieEffectCpuRgba8ApiV1>() + align_of::<RuvieEffectCpuRgba8ApiV1>()];
    // SAFETY: The allocation includes the offset byte and a complete
    // extension table.
    let extension_pointer = unsafe { extension_storage.as_mut_ptr().add(1) };
    // SAFETY: Same bounded unaligned fixture construction as the base table.
    unsafe {
        extension_pointer
            .cast::<RuvieEffectCpuRgba8ApiV1>()
            .write_unaligned(extension)
    };
    MISALIGNED_EXTENSION_POINTER.store(
        extension_pointer as usize,
        std::sync::atomic::Ordering::SeqCst,
    );
    let library = RuntimeLibrary {
        api: RuviePluginApiV1 {
            abi_version: RUVIE_PLUGIN_ABI_V1,
            struct_size: size_of::<RuviePluginApiV1>(),
            context: std::ptr::null_mut(),
            descriptor_json: None,
            invoke_json: None,
            free_buffer: None,
            query_extension: Some(misaligned_extension_query),
        },
        _library: current_process_library(),
    };
    let Err(error) = library.effect_cpu_rgba8_extension() else {
        return Err(std::io::Error::other(
            "a misaligned extension must fail before callback validation",
        )
        .into());
    };
    MISALIGNED_EXTENSION_POINTER.store(0, std::sync::atomic::Ordering::SeqCst);
    assert!(error.to_string().contains("misaligned ABI table"));
    assert_eq!(
        MISALIGNED_TABLE_CALLBACKS.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a callback pointer from a misaligned extension table must never execute"
    );
    Ok(())
}

unsafe extern "C" fn free_test_frame(
    _context: *mut std::ffi::c_void,
    frame: RuvieOwnedRgba8FrameV1,
) {
    FREED_TEST_FRAMES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    // SAFETY: Each test frame below is allocated by `RuvieBuffer::from_vec`
    // and this callback receives the exact buffer once.
    unsafe { ruvie_plugin_api::free_owned_buffer(frame.pixels) };
}

fn owned_test_frame(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    stride_bytes: usize,
) -> RuvieOwnedRgba8FrameV1 {
    RuvieOwnedRgba8FrameV1 {
        struct_size: size_of::<RuvieOwnedRgba8FrameV1>(),
        width,
        height,
        stride_bytes,
        alpha_mode: ALPHA_MODE_STRAIGHT_V1,
        color_profile: COLOR_PROFILE_SRGB_V1,
        pixels: RuvieBuffer::from_vec(pixels),
    }
}

#[test]
fn malformed_and_overflowing_rgba8_outputs_are_rejected_and_reclaimed() {
    FREED_TEST_FRAMES.store(0, std::sync::atomic::Ordering::SeqCst);

    let mut wrong_alpha = owned_test_frame(vec![1, 2, 3, 255], 1, 1, 4);
    wrong_alpha.alpha_mode = 99;
    assert!(
        copy_owned_frame(std::ptr::null_mut(), Some(free_test_frame), wrong_alpha)
            .expect_err("unknown alpha semantics must be rejected")
            .to_string()
            .contains("alpha mode")
    );

    let overflowing = owned_test_frame(vec![0; 4], 1, 2, usize::MAX);
    assert!(
        copy_owned_frame(std::ptr::null_mut(), Some(free_test_frame), overflowing)
            .expect_err("stride times height overflow must be rejected before reading")
            .to_string()
            .contains("overflow")
    );
    assert_eq!(
        FREED_TEST_FRAMES.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "semantic rejection must still return plugin allocations exactly once"
    );

    let unreclaimable = RuvieOwnedRgba8FrameV1 {
        pixels: RuvieBuffer {
            ptr: std::ptr::null_mut(),
            len: 4,
            capacity: 4,
        },
        ..RuvieOwnedRgba8FrameV1::empty()
    };
    assert!(
        copy_owned_frame(std::ptr::null_mut(), Some(free_test_frame), unreclaimable)
            .expect_err("a null non-empty buffer cannot be dereferenced or freed")
            .to_string()
            .contains("unreclaimable")
    );
    assert_eq!(
        FREED_TEST_FRAMES.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

#[test]
fn rgba8_and_metadata_bounds_fail_before_large_allocation() {
    assert!(
        validate_rgba8_layout(1, 2, usize::MAX, 0)
            .expect_err("multiplication overflow must be explicit")
            .to_string()
            .contains("overflow")
    );
    let stride = usize::try_from(MAX_CPU_RGBA8_DIMENSION_V1).unwrap_or(usize::MAX) * 4;
    let oversized_len = stride * 5_000;
    assert!(oversized_len > MAX_CPU_RGBA8_FRAME_BYTES_V1);
    assert!(
        validate_rgba8_layout(MAX_CPU_RGBA8_DIMENSION_V1, 5_000, stride, oversized_len)
            .expect_err("the ABI frame byte cap must be enforced")
            .to_string()
            .contains("bounded layout")
    );

    let invalid_metadata = RuvieAssetMetadataV1 {
        kind: ASSET_KIND_VIDEO_V1,
        present_fields: ASSET_METADATA_FPS_V1 | ASSET_METADATA_TIME_BASE_V1,
        fps: f64::NAN,
        time_base_numerator: 1,
        time_base_denominator: 0,
        ..RuvieAssetMetadataV1::default()
    };
    assert!(
        metadata_from_wire(invalid_metadata)
            .expect_err("non-finite FPS must be rejected")
            .to_string()
            .contains("FPS")
    );
}

#[test]
fn panic_status_and_message_cross_the_boundary_without_becoming_no_output() {
    let library = RuntimeLibrary {
        api: RuviePluginApiV1 {
            abi_version: RUVIE_PLUGIN_ABI_V1,
            struct_size: size_of::<RuviePluginApiV1>(),
            context: std::ptr::null_mut(),
            descriptor_json: None,
            invoke_json: None,
            free_buffer: Some(test_free_buffer),
            query_extension: None,
        },
        _library: current_process_library(),
    };
    let error = library
        .consume_extension_result(RuvieExtensionResultV1::error(
            ruvie_plugin_api::STATUS_PANIC,
            "fixture callback panicked",
        ))
        .expect_err("STATUS_PANIC must remain a real plugin failure")
        .to_string();
    assert!(error.contains("status 3"));
    assert!(error.contains("fixture callback panicked"));
}
