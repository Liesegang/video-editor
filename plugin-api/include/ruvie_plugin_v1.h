#ifndef RUVIE_PLUGIN_V1_H
#define RUVIE_PLUGIN_V1_H

#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#define RUVIE_PLUGIN_EXPORT __declspec(dllexport)
#else
#define RUVIE_PLUGIN_EXPORT __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define RUVIE_PLUGIN_ABI_V1 1u
#define RUVIE_STATUS_OK 0u
#define RUVIE_STATUS_PLUGIN_ERROR 1u
#define RUVIE_STATUS_INVALID_REQUEST 2u
#define RUVIE_STATUS_PANIC 3u
#define RUVIE_STATUS_UNSUPPORTED 4u

#define RUVIE_EFFECT_CPU_RGBA8_EXTENSION_V1 "ruvie.effect.cpu-rgba8.v1"
#define RUVIE_EFFECT_CATEGORY "effect"
#define RUVIE_EFFECT_PROCESS_CPU_RGBA8_V1 "effect.process.cpu-rgba8.v1"
#define RUVIE_LOADER_CPU_RGBA8_EXTENSION_V1 "ruvie.loader.cpu-rgba8.v1"
#define RUVIE_LOADER_CATEGORY "loader"
#define RUVIE_LOADER_OPEN_V1 "loader.open.v1"
#define RUVIE_LOADER_LOAD_CPU_RGBA8_V1 "loader.load.cpu-rgba8.v1"
#define RUVIE_CPU_RGBA8_MAX_DIMENSION_V1 32768u
#define RUVIE_CPU_RGBA8_MAX_FRAME_BYTES_V1 (512u * 1024u * 1024u)
#define RUVIE_LOADER_MAX_STREAMS_V1 64u
#define RUVIE_ALPHA_MODE_STRAIGHT_V1 1u
#define RUVIE_COLOR_PROFILE_SRGB_V1 1u

#define RUVIE_PROPERTY_VALUE_NUMBER_V1 1u
#define RUVIE_PROPERTY_VALUE_INTEGER_V1 2u
#define RUVIE_PROPERTY_VALUE_STRING_V1 3u
#define RUVIE_PROPERTY_VALUE_BOOLEAN_V1 4u
#define RUVIE_PROPERTY_VALUE_VEC2_V1 5u
#define RUVIE_PROPERTY_VALUE_VEC3_V1 6u
#define RUVIE_PROPERTY_VALUE_VEC4_V1 7u
#define RUVIE_PROPERTY_VALUE_COLOR_V1 8u

#define RUVIE_LOAD_REQUEST_IMAGE_V1 1u
#define RUVIE_LOAD_REQUEST_VIDEO_FRAME_V1 2u
#define RUVIE_ASSET_KIND_IMAGE_V1 1u
#define RUVIE_ASSET_KIND_VIDEO_V1 2u
#define RUVIE_ASSET_METADATA_DURATION_V1 (1u << 0)
#define RUVIE_ASSET_METADATA_FPS_V1 (1u << 1)
#define RUVIE_ASSET_METADATA_DIMENSIONS_V1 (1u << 2)
#define RUVIE_ASSET_METADATA_STREAM_INDEX_V1 (1u << 3)
#define RUVIE_ASSET_METADATA_FRAME_COUNT_V1 (1u << 4)
#define RUVIE_ASSET_METADATA_TIME_BASE_V1 (1u << 5)

/* JSON control-plane names standardized by ABI v1. */
#define RUVIE_EFFECTOR_CATEGORY "effector"
#define RUVIE_EFFECTOR_EVALUATE_V1 "effector.evaluate.v1"
#define RUVIE_PROPERTY_CATEGORY "property"
#define RUVIE_PROPERTY_EVALUATE_V1 "property.evaluate.v1"
#define RUVIE_STYLE_CATEGORY "style"
#define RUVIE_STYLE_EVALUATE_V1 "style.evaluate.v1"
#define RUVIE_STYLE_MAX_DASH_INTERVALS_V1 1024u
#define RUVIE_DECORATOR_CATEGORY "decorator"
#define RUVIE_DECORATOR_EVALUATE_V1 "decorator.evaluate.v1"
#define RUVIE_EFFECTOR_TARGET_BLOCK "block"
#define RUVIE_EFFECTOR_TARGET_LINE "line"
#define RUVIE_EFFECTOR_TARGET_CHAR "char"
#define RUVIE_STYLE_OUTPUT_NO_OUTPUT "no_output"
#define RUVIE_STYLE_OUTPUT_FILL "fill"
#define RUVIE_STYLE_OUTPUT_STROKE "stroke"
#define RUVIE_DECORATOR_OUTPUT_NO_OUTPUT "no_output"
#define RUVIE_DECORATOR_OUTPUT_BACKPLATE "backplate"
#define RUVIE_DECORATOR_TARGET_BLOCK "block"
#define RUVIE_DECORATOR_TARGET_LINE "line"
#define RUVIE_DECORATOR_TARGET_CHAR "char"

typedef struct RuvieBytesView {
    const uint8_t *ptr;
    size_t len;
} RuvieBytesView;

typedef struct RuvieBuffer {
    uint8_t *ptr;
    size_t len;
    size_t capacity;
} RuvieBuffer;

typedef struct RuvieCallResult {
    uint32_t status;
    RuvieBuffer buffer;
} RuvieCallResult;

typedef struct RuvieExtensionResultV1 {
    uint32_t status;
    RuvieBuffer message;
} RuvieExtensionResultV1;

typedef struct RuviePropertyValueViewV1 {
    RuvieBytesView name;
    uint32_t value_type;
    double number;
    int64_t integer;
    RuvieBytesView bytes;
    double vector[4];
    uint8_t color[4];
} RuviePropertyValueViewV1;

typedef struct RuviePropertyMapViewV1 {
    const RuviePropertyValueViewV1 *ptr;
    size_t len;
} RuviePropertyMapViewV1;

typedef struct RuvieRgba8FrameViewV1 {
    size_t struct_size;
    uint32_t width;
    uint32_t height;
    size_t stride_bytes;
    uint32_t alpha_mode;
    uint32_t color_profile;
    RuvieBytesView pixels;
} RuvieRgba8FrameViewV1;

typedef struct RuvieOwnedRgba8FrameV1 {
    size_t struct_size;
    uint32_t width;
    uint32_t height;
    size_t stride_bytes;
    uint32_t alpha_mode;
    uint32_t color_profile;
    RuvieBuffer pixels;
} RuvieOwnedRgba8FrameV1;

typedef RuvieExtensionResultV1 (*RuvieEffectCreateInstanceV1Fn)(
    void *context, RuvieBytesView component_id,
    RuviePropertyMapViewV1 properties, uint64_t *out_instance);
typedef RuvieExtensionResultV1 (*RuvieEffectProcessCpuRgba8V1Fn)(
    void *context, uint64_t instance, double time_seconds,
    const RuvieRgba8FrameViewV1 *input, RuvieOwnedRgba8FrameV1 *output);
typedef void (*RuvieEffectReleaseInstanceV1Fn)(void *context,
                                               uint64_t instance);
typedef void (*RuvieFreeRgba8FrameV1Fn)(void *context,
                                        RuvieOwnedRgba8FrameV1 frame);

typedef struct RuvieEffectCpuRgba8ApiV1 {
    uint32_t abi_version;
    size_t struct_size;
    void *context;
    RuvieEffectCreateInstanceV1Fn create_instance;
    RuvieEffectProcessCpuRgba8V1Fn process;
    RuvieEffectReleaseInstanceV1Fn release_instance;
    RuvieFreeRgba8FrameV1Fn free_frame;
} RuvieEffectCpuRgba8ApiV1;

typedef struct RuvieAssetMetadataV1 {
    uint32_t kind;
    uint32_t present_fields;
    double duration_seconds;
    double fps;
    uint32_t width;
    uint32_t height;
    uint32_t stream_index;
    uint64_t frame_count;
    int32_t time_base_numerator;
    int32_t time_base_denominator;
} RuvieAssetMetadataV1;

typedef struct RuvieLoaderRequestV1 {
    size_t struct_size;
    uint32_t request_kind;
    RuvieBytesView path;
    double source_time;
    uint32_t has_stream_index;
    uint32_t stream_index;
    RuvieBytesView input_color_space;
    RuvieBytesView output_color_space;
} RuvieLoaderRequestV1;

typedef RuvieExtensionResultV1 (*RuvieLoaderOpenV1Fn)(
    void *context, RuvieBytesView component_id, RuvieBytesView path,
    RuvieAssetMetadataV1 *metadata, size_t metadata_capacity,
    size_t *out_metadata_len);
typedef RuvieExtensionResultV1 (*RuvieLoaderLoadCpuRgba8V1Fn)(
    void *context, RuvieBytesView component_id,
    const RuvieLoaderRequestV1 *request, RuvieOwnedRgba8FrameV1 *output);

typedef struct RuvieLoaderCpuRgba8ApiV1 {
    uint32_t abi_version;
    size_t struct_size;
    void *context;
    RuvieLoaderOpenV1Fn open;
    RuvieLoaderLoadCpuRgba8V1Fn load;
    RuvieFreeRgba8FrameV1Fn free_frame;
} RuvieLoaderCpuRgba8ApiV1;

typedef RuvieCallResult (*RuvieDescriptorJsonFn)(void *context);
typedef RuvieCallResult (*RuvieInvokeJsonFn)(void *context,
                                             RuvieBytesView request);
typedef void (*RuvieFreeBufferFn)(void *context, RuvieBuffer buffer);
typedef const void *(*RuvieQueryExtensionFn)(void *context,
                                             RuvieBytesView extension_name);

typedef struct RuviePluginApiV1 {
    uint32_t abi_version;
    size_t struct_size;
    void *context;
    RuvieDescriptorJsonFn descriptor_json;
    RuvieInvokeJsonFn invoke_json;
    RuvieFreeBufferFn free_buffer;
    RuvieQueryExtensionFn query_extension; /* nullable */
} RuviePluginApiV1;

/* A plugin exports exactly this symbol for the v1 base/control-plane ABI. */
RUVIE_PLUGIN_EXPORT const RuviePluginApiV1 *ruvie_plugin_entry_v1(void);

#ifdef __cplusplus
}
#endif

#endif /* RUVIE_PLUGIN_V1_H */
