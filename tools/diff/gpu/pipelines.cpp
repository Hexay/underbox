// Pipeline state at creation, with graphics pipeline libraries merged into the linked pipeline.
#include "capture.h"
#include <cstring>

using namespace gpu;
using Lock = std::lock_guard<std::mutex>;

namespace gpu {
std::unordered_map<uint64_t, std::shared_ptr<PipeInfo>>& pipes() { static std::unordered_map<uint64_t, std::shared_ptr<PipeInfo>> m; return m; }
}

static const void* find_next(const void* pNext, VkStructureType t) {
    for (auto p = (const VkBaseInStructure*)pNext; p; p = p->pNext)
        if (p->sType == t) return p;
    return nullptr;
}

static Stage stage_of(const VkPipelineShaderStageCreateInfo& s) {
    Stage st{s.stage, {0, 0, ""}, s.pName ? s.pName : ""};
    if (s.module) {
        auto it = objs().shaders.find(h64(s.module));
        if (it != objs().shaders.end()) st.sh = it->second;
    } else if (auto m = (const VkShaderModuleCreateInfo*)find_next(s.pNext, VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO)) {
        st.sh = shader_info(m->pCode, m->codeSize);
    }
    return st;
}

static void merge(PipeInfo& p, const PipeInfo& lib) {
    uint32_t parts = lib.parts;
    if (parts & P_VI) { p.vb = lib.vb; p.va = lib.va; p.topo = lib.topo; p.restart = lib.restart; }
    if (parts & P_PRE) p.rs = lib.rs;
    if (parts & P_FS) p.ds = lib.ds;
    if (parts & P_OUT) {
        p.blend = lib.blend;
        memcpy(p.bc, lib.bc, sizeof p.bc);
        p.samples = lib.samples;
        p.a2c = lib.a2c;
        p.colorFmts = lib.colorFmts;
    }
    if (lib.depthFmt) p.depthFmt = lib.depthFmt;
    p.stages.insert(p.stages.end(), lib.stages.begin(), lib.stages.end());
    for (auto s : lib.dyn)
        if (!p.has_dyn(s)) p.dyn.push_back(s);
    if (!p.layout) p.layout = lib.layout;
    p.parts |= parts;
}

static PipeInfo parse(const VkGraphicsPipelineCreateInfo& ci) {
    PipeInfo p;
    uint32_t fromLibs = 0;
    if (auto li = (const VkPipelineLibraryCreateInfoKHR*)find_next(ci.pNext, VK_STRUCTURE_TYPE_PIPELINE_LIBRARY_CREATE_INFO_KHR)) {
        for (uint32_t i = 0; i < li->libraryCount; i++) {
            auto it = pipes().find(h64(li->pLibraries[i]));
            if (it == pipes().end()) continue;
            merge(p, *it->second);
            fromLibs |= it->second->parts;
        }
    }
    uint32_t here = P_ALL & ~fromLibs;
    if (auto g = (const VkGraphicsPipelineLibraryCreateInfoEXT*)find_next(ci.pNext, VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_LIBRARY_CREATE_INFO_EXT))
        here = g->flags & P_ALL;
    if (ci.pDynamicState)
        for (uint32_t i = 0; i < ci.pDynamicState->dynamicStateCount; i++)
            if (!p.has_dyn(ci.pDynamicState->pDynamicStates[i])) p.dyn.push_back(ci.pDynamicState->pDynamicStates[i]);
    for (uint32_t i = 0; i < ci.stageCount; i++) {
        uint32_t part = ci.pStages[i].stage == VK_SHADER_STAGE_FRAGMENT_BIT ? P_FS : P_PRE;
        if (here & part) p.stages.push_back(stage_of(ci.pStages[i]));
    }
    if ((here & P_VI) && ci.pVertexInputState && !p.has_dyn(VK_DYNAMIC_STATE_VERTEX_INPUT_EXT)) {
        auto& vi = *ci.pVertexInputState;
        p.vb.assign(vi.pVertexBindingDescriptions, vi.pVertexBindingDescriptions + vi.vertexBindingDescriptionCount);
        p.va.assign(vi.pVertexAttributeDescriptions, vi.pVertexAttributeDescriptions + vi.vertexAttributeDescriptionCount);
    }
    if ((here & P_VI) && ci.pInputAssemblyState) { p.topo = ci.pInputAssemblyState->topology; p.restart = ci.pInputAssemblyState->primitiveRestartEnable; }
    if ((here & P_PRE) && ci.pRasterizationState) { p.rs = *ci.pRasterizationState; p.rs.pNext = nullptr; }
    if ((here & P_FS) && ci.pDepthStencilState) { p.ds = *ci.pDepthStencilState; p.ds.pNext = nullptr; }
    if ((here & (P_FS | P_OUT)) && ci.pMultisampleState) { p.samples = ci.pMultisampleState->rasterizationSamples; p.a2c = ci.pMultisampleState->alphaToCoverageEnable; }
    if ((here & P_OUT) && ci.pColorBlendState) {
        auto& cb = *ci.pColorBlendState;
        p.blend.clear();
        for (uint32_t i = 0; cb.pAttachments && i < cb.attachmentCount; i++) {
            auto& a = cb.pAttachments[i];
            p.blend.push_back({a.blendEnable, a.srcColorBlendFactor, a.dstColorBlendFactor, a.srcAlphaBlendFactor,
                               a.dstAlphaBlendFactor, a.colorBlendOp, a.alphaBlendOp, a.colorWriteMask});
        }
        memcpy(p.bc, cb.blendConstants, sizeof p.bc);
    }
    if (here & (P_FS | P_OUT)) {
        if (auto r = (const VkPipelineRenderingCreateInfo*)find_next(ci.pNext, VK_STRUCTURE_TYPE_PIPELINE_RENDERING_CREATE_INFO)) {
            if (here & P_OUT) p.colorFmts.assign(r->pColorAttachmentFormats, r->pColorAttachmentFormats + r->colorAttachmentCount);
            if (r->depthAttachmentFormat) p.depthFmt = r->depthAttachmentFormat;
        }
    }
    if (ci.layout) p.layout = ci.layout;
    p.parts |= here;
    return p;
}

VkResult hk_CreateGraphicsPipelines(VkDevice dev, VkPipelineCache cache, uint32_t n, const VkGraphicsPipelineCreateInfo* ci,
                                    const VkAllocationCallbacks* a, VkPipeline* out) {
    VkResult r = device(dev)->CreateGraphicsPipelines(dev, cache, n, ci, a, out);
    Lock l(mu());
    for (uint32_t i = 0; i < n; i++)
        if (out[i]) pipes()[h64(out[i])] = std::make_shared<PipeInfo>(parse(ci[i]));
    return r;
}

VkResult hk_CreateComputePipelines(VkDevice dev, VkPipelineCache cache, uint32_t n, const VkComputePipelineCreateInfo* ci,
                                   const VkAllocationCallbacks* a, VkPipeline* out) {
    VkResult r = device(dev)->CreateComputePipelines(dev, cache, n, ci, a, out);
    Lock l(mu());
    for (uint32_t i = 0; i < n; i++) {
        if (!out[i]) continue;
        auto p = std::make_shared<PipeInfo>();
        p->compute = true;
        p->stages.push_back(stage_of(ci[i].stage));
        p->layout = ci[i].layout;
        pipes()[h64(out[i])] = p;
    }
    return r;
}
