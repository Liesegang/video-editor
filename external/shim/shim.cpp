#include <OpenColorIO/OpenColorIO.h>
#include <iostream>
#include <vector>
#include <string>
#include <cstring>

namespace OCIO = OCIO_NAMESPACE;

struct OcioContext {
    OCIO::ConstConfigRcPtr config;
};

struct OcioProcessor {
    OCIO::ConstProcessorRcPtr processor;
    OCIO::ConstCPUProcessorRcPtr cpu_processor;
};

extern "C" {

    __declspec(dllexport) OcioContext* ocio_create_context() {
        try {
            auto config = OCIO::Config::CreateFromEnv();
            return new OcioContext{ config };
        } catch (...) {
            return nullptr;
        }
    }

    __declspec(dllexport) void ocio_destroy_context(OcioContext* ctx) {
        if (ctx) delete ctx;
    }

    __declspec(dllexport) int ocio_get_num_colorspaces(OcioContext* ctx) {
        if (!ctx || !ctx->config) return 0;
        return ctx->config->getNumColorSpaces();
    }

    __declspec(dllexport) const char* ocio_get_colorspace_name(OcioContext* ctx, int index) {
        if (!ctx || !ctx->config) return nullptr;
        return ctx->config->getColorSpaceNameByIndex(index);
    }

    __declspec(dllexport) OcioProcessor* ocio_create_processor(OcioContext* ctx, const char* src, const char* dst) {
        if (!ctx || !ctx->config) return nullptr;
        try {
            auto processor = ctx->config->getProcessor(src, dst);
            auto cpu_processor = processor->getDefaultCPUProcessor();
            return new OcioProcessor{ processor, cpu_processor };
        } catch (...) {
            return nullptr;
        }
    }

    __declspec(dllexport) void ocio_destroy_processor(OcioProcessor* proc) {
        if (proc) delete proc;
    }

    __declspec(dllexport) void ocio_apply_transform(OcioProcessor* proc, float* pixel, int count) {
        if (!proc || !proc->cpu_processor) return;
        
        OCIO::PackedImageDesc img(pixel, count, 1, 4); 
        proc->cpu_processor->apply(img);
    }
}
