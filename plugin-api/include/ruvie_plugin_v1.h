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

/* JSON control-plane names standardized by ABI v1. */
#define RUVIE_EFFECTOR_CATEGORY "effector"
#define RUVIE_EFFECTOR_EVALUATE_V1 "effector.evaluate.v1"
#define RUVIE_EFFECTOR_TARGET_BLOCK "block"
#define RUVIE_EFFECTOR_TARGET_LINE "line"
#define RUVIE_EFFECTOR_TARGET_CHAR "char"

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
