// Object tracking: what the capture needs to describe images, buffers, shaders and passes later.
#include "layer.h"
#include <cstdlib>
#include <cstring>

using namespace gpu;
using Lock = std::lock_guard<std::mutex>;

namespace gpu {

bool host_visible(Device* d, uint32_t type) {
    return type < d->mem.memoryTypeCount && (d->mem.memoryTypes[type].propertyFlags & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT);
}

std::vector<uint8_t> read_buffer(Device* d, VkBuffer b, VkDeviceSize off, VkDeviceSize n) {
    auto& o = objs();
    auto bi = o.buffers.find(h64(b));
    if (bi == o.buffers.end() || !bi->second.mem) return {};
    if (n == VK_WHOLE_SIZE) n = off < bi->second.size ? bi->second.size - off : 0;
    if (off + n > bi->second.size) return {};
    auto mi = o.mems.find(h64(bi->second.mem));
    if (mi == o.mems.end() || !host_visible(d, mi->second.type)) return {};
    VkDeviceSize at = bi->second.memOffset + off;
    std::vector<uint8_t> out(n);
    MemInfo& m = mi->second;
    if (m.mapped) {
        if (at < m.mapOffset) return {};
        memcpy(out.data(), (uint8_t*)m.mapped + (at - m.mapOffset), n);
        return out;
    }
    void* p = nullptr;
    if (d->MapMemory(d->dev, bi->second.mem, 0, VK_WHOLE_SIZE, 0, &p) != VK_SUCCESS) return {};
    memcpy(out.data(), (uint8_t*)p + at, n);
    d->UnmapMemory(d->dev, bi->second.mem);
    return out;
}

ShaderInfo shader_info(const uint32_t* code, size_t bytes) {
    ShaderInfo s{fnv(code, bytes), bytes, ""};
    size_t n = bytes / 4;
    for (size_t i = 5; i < n;) {
        uint32_t wc = code[i] >> 16, op = code[i] & 0xffff;
        if (!wc) break;
        // OpString: the first one is normally the source file the compiler embedded.
        if (op == 7 && wc > 2 && s.src.empty()) s.src.assign((const char*)&code[i + 2], strnlen((const char*)&code[i + 2], (wc - 2) * 4));
        i += wc;
    }
    return s;
}

uint32_t texel_bytes(VkFormat f) {
    switch (f) {
    case VK_FORMAT_R8_UNORM: case VK_FORMAT_R8_UINT: case VK_FORMAT_S8_UINT: return 1;
    case VK_FORMAT_R8G8_UNORM: case VK_FORMAT_R16_SFLOAT: case VK_FORMAT_R16_UNORM: case VK_FORMAT_D16_UNORM: return 2;
    case VK_FORMAT_R8G8B8A8_UNORM: case VK_FORMAT_R8G8B8A8_SRGB: case VK_FORMAT_B8G8R8A8_UNORM: case VK_FORMAT_B8G8R8A8_SRGB:
    case VK_FORMAT_A2B10G10R10_UNORM_PACK32: case VK_FORMAT_A2R10G10B10_UNORM_PACK32: case VK_FORMAT_B10G11R11_UFLOAT_PACK32:
    case VK_FORMAT_R32_SFLOAT: case VK_FORMAT_R32_UINT: case VK_FORMAT_R16G16_SFLOAT: case VK_FORMAT_R16G16_UNORM:
    case VK_FORMAT_D32_SFLOAT: case VK_FORMAT_X8_D24_UNORM_PACK32: case VK_FORMAT_D24_UNORM_S8_UINT:
    case VK_FORMAT_D32_SFLOAT_S8_UINT: case VK_FORMAT_E5B9G9R9_UFLOAT_PACK32: return 4;
    case VK_FORMAT_R16G16B16A16_SFLOAT: case VK_FORMAT_R16G16B16A16_UNORM: case VK_FORMAT_R32G32_SFLOAT: return 8;
    case VK_FORMAT_R32G32B32A32_SFLOAT: return 16;
    default: return 0;
    }
}

}  // namespace gpu

static const bool g_dump = getenv("UNDERBOX_GPU_DUMP") && *getenv("UNDERBOX_GPU_DUMP");
static constexpr VkImageUsageFlags kAttach = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT;

VkResult hk_CreateImage(VkDevice dev, const VkImageCreateInfo* ci, const VkAllocationCallbacks* a, VkImage* out) {
    Device* d = device(dev);
    VkImageCreateInfo c = *ci;
    // Render-target dumps copy attachments out; they need to be transfer sources.
    if (g_dump && (c.usage & kAttach)) c.usage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    VkResult r = d->CreateImage(dev, &c, a, out);
    if (r == VK_SUCCESS) {
        Lock l(mu());
        objs().images[h64(*out)] = {ci->imageType, ci->format, ci->extent, ci->mipLevels, ci->arrayLayers, ci->samples};
    }
    return r;
}

void hk_DestroyImage(VkDevice dev, VkImage img, const VkAllocationCallbacks* a) {
    { Lock l(mu()); objs().images.erase(h64(img)); objs().names.erase(h64(img)); }
    device(dev)->DestroyImage(dev, img, a);
}

VkResult hk_CreateImageView(VkDevice dev, const VkImageViewCreateInfo* ci, const VkAllocationCallbacks* a, VkImageView* out) {
    VkResult r = device(dev)->CreateImageView(dev, ci, a, out);
    if (r == VK_SUCCESS) { Lock l(mu()); objs().views[h64(*out)] = {ci->image, ci->format, ci->viewType, ci->subresourceRange}; }
    return r;
}

VkResult hk_CreateSampler(VkDevice dev, const VkSamplerCreateInfo* ci, const VkAllocationCallbacks* a, VkSampler* out) {
    VkResult r = device(dev)->CreateSampler(dev, ci, a, out);
    if (r == VK_SUCCESS) { Lock l(mu()); auto& s = objs().samplers[h64(*out)]; s = *ci; s.pNext = nullptr; }
    return r;
}

VkResult hk_CreateBuffer(VkDevice dev, const VkBufferCreateInfo* ci, const VkAllocationCallbacks* a, VkBuffer* out) {
    VkResult r = device(dev)->CreateBuffer(dev, ci, a, out);
    if (r == VK_SUCCESS) { Lock l(mu()); objs().buffers[h64(*out)] = {ci->size, ci->usage}; }
    return r;
}

void hk_DestroyBuffer(VkDevice dev, VkBuffer b, const VkAllocationCallbacks* a) {
    { Lock l(mu()); objs().buffers.erase(h64(b)); objs().names.erase(h64(b)); }
    device(dev)->DestroyBuffer(dev, b, a);
}

static void bind(VkBuffer b, VkDeviceMemory m, VkDeviceSize off) {
    auto it = objs().buffers.find(h64(b));
    if (it != objs().buffers.end()) { it->second.mem = m; it->second.memOffset = off; }
}

VkResult hk_BindBufferMemory(VkDevice dev, VkBuffer b, VkDeviceMemory m, VkDeviceSize off) {
    VkResult r = device(dev)->BindBufferMemory(dev, b, m, off);
    if (r == VK_SUCCESS) { Lock l(mu()); bind(b, m, off); }
    return r;
}

VkResult hk_BindBufferMemory2(VkDevice dev, uint32_t n, const VkBindBufferMemoryInfo* infos) {
    VkResult r = device(dev)->BindBufferMemory2(dev, n, infos);
    if (r == VK_SUCCESS) { Lock l(mu()); for (uint32_t i = 0; i < n; i++) bind(infos[i].buffer, infos[i].memory, infos[i].memoryOffset); }
    return r;
}

VkResult hk_AllocateMemory(VkDevice dev, const VkMemoryAllocateInfo* ai, const VkAllocationCallbacks* a, VkDeviceMemory* out) {
    VkResult r = device(dev)->AllocateMemory(dev, ai, a, out);
    if (r == VK_SUCCESS) { Lock l(mu()); objs().mems[h64(*out)] = {ai->allocationSize, ai->memoryTypeIndex}; }
    return r;
}

void hk_FreeMemory(VkDevice dev, VkDeviceMemory m, const VkAllocationCallbacks* a) {
    { Lock l(mu()); objs().mems.erase(h64(m)); }
    device(dev)->FreeMemory(dev, m, a);
}

static void mapped(VkDeviceMemory m, VkDeviceSize off, void* p) {
    auto it = objs().mems.find(h64(m));
    if (it != objs().mems.end()) { it->second.mapped = p; it->second.mapOffset = off; }
}

VkResult hk_MapMemory(VkDevice dev, VkDeviceMemory m, VkDeviceSize off, VkDeviceSize size, VkMemoryMapFlags f, void** p) {
    Lock l(mu());
    VkResult r = device(dev)->MapMemory(dev, m, off, size, f, p);
    if (r == VK_SUCCESS) mapped(m, off, *p);
    return r;
}

void hk_UnmapMemory(VkDevice dev, VkDeviceMemory m) {
    Lock l(mu());
    device(dev)->UnmapMemory(dev, m);
    mapped(m, 0, nullptr);
}

VkResult hk_MapMemory2(VkDevice dev, const VkMemoryMapInfo* info, void** p) {
    Lock l(mu());
    VkResult r = device(dev)->MapMemory2(dev, info, p);
    if (r == VK_SUCCESS) mapped(info->memory, info->offset, *p);
    return r;
}

VkResult hk_UnmapMemory2(VkDevice dev, const VkMemoryUnmapInfo* info) {
    Lock l(mu());
    VkResult r = device(dev)->UnmapMemory2(dev, info);
    mapped(info->memory, 0, nullptr);
    return r;
}

VkResult hk_CreateSwapchainKHR(VkDevice dev, const VkSwapchainCreateInfoKHR* ci, const VkAllocationCallbacks* a, VkSwapchainKHR* out) {
    VkSwapchainCreateInfoKHR c = *ci;
    if (g_dump) c.imageUsage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    VkResult r = device(dev)->CreateSwapchainKHR(dev, &c, a, out);
    if (r != VK_SUCCESS && g_dump) r = device(dev)->CreateSwapchainKHR(dev, ci, a, out);
    if (r == VK_SUCCESS) { Lock l(mu()); auto& s = objs().swapchains[h64(*out)]; s = *ci; s.pNext = nullptr; }
    return r;
}

VkResult hk_GetSwapchainImagesKHR(VkDevice dev, VkSwapchainKHR sc, uint32_t* n, VkImage* imgs) {
    VkResult r = device(dev)->GetSwapchainImagesKHR(dev, sc, n, imgs);
    if (imgs && (r == VK_SUCCESS || r == VK_INCOMPLETE)) {
        Lock l(mu());
        auto& s = objs().swapchains[h64(sc)];
        for (uint32_t i = 0; i < *n; i++)
            objs().images[h64(imgs[i])] = {VK_IMAGE_TYPE_2D, s.imageFormat, {s.imageExtent.width, s.imageExtent.height, 1}, 1, s.imageArrayLayers, VK_SAMPLE_COUNT_1_BIT, true};
    }
    return r;
}

VkResult hk_SetDebugUtilsObjectNameEXT(VkDevice dev, const VkDebugUtilsObjectNameInfoEXT* info) {
    if (info->pObjectName) { Lock l(mu()); objs().names[info->objectHandle] = info->pObjectName; }
    return device(dev)->SetDebugUtilsObjectNameEXT(dev, info);
}

VkResult hk_CreateShaderModule(VkDevice dev, const VkShaderModuleCreateInfo* ci, const VkAllocationCallbacks* a, VkShaderModule* out) {
    VkResult r = device(dev)->CreateShaderModule(dev, ci, a, out);
    if (r == VK_SUCCESS) { auto s = shader_info(ci->pCode, ci->codeSize); Lock l(mu()); objs().shaders[h64(*out)] = s; }
    return r;
}

VkResult hk_CreateDescriptorSetLayout(VkDevice dev, const VkDescriptorSetLayoutCreateInfo* ci, const VkAllocationCallbacks* a, VkDescriptorSetLayout* out) {
    VkResult r = device(dev)->CreateDescriptorSetLayout(dev, ci, a, out);
    if (r != VK_SUCCESS) return r;
    SetLayoutInfo s;
    for (uint32_t i = 0; i < ci->bindingCount; i++) {
        auto& b = ci->pBindings[i];
        s.bindings.push_back({b.binding, b.descriptorType, b.descriptorCount, b.stageFlags});
    }
    std::sort(s.bindings.begin(), s.bindings.end(), [](auto& x, auto& y) { return x.binding < y.binding; });
    Lock l(mu());
    objs().setLayouts[h64(*out)] = std::move(s);
    return r;
}

VkResult hk_CreatePipelineLayout(VkDevice dev, const VkPipelineLayoutCreateInfo* ci, const VkAllocationCallbacks* a, VkPipelineLayout* out) {
    VkResult r = device(dev)->CreatePipelineLayout(dev, ci, a, out);
    if (r != VK_SUCCESS) return r;
    Lock l(mu());
    objs().pipeLayouts[h64(*out)] = {{ci->pSetLayouts, ci->pSetLayouts + ci->setLayoutCount},
                                     {ci->pPushConstantRanges, ci->pPushConstantRanges + ci->pushConstantRangeCount}};
    return r;
}

VkResult hk_CreateRenderPass(VkDevice dev, const VkRenderPassCreateInfo* ci, const VkAllocationCallbacks* a, VkRenderPass* out) {
    VkResult r = device(dev)->CreateRenderPass(dev, ci, a, out);
    if (r != VK_SUCCESS) return r;
    RenderPassInfo p;
    for (uint32_t i = 0; i < ci->attachmentCount; i++) {
        auto& x = ci->pAttachments[i];
        p.atts.push_back({x.format, x.samples, x.loadOp, x.storeOp, x.stencilLoadOp, x.finalLayout});
    }
    Lock l(mu());
    objs().passes[h64(*out)] = std::move(p);
    return r;
}

VkResult hk_CreateRenderPass2(VkDevice dev, const VkRenderPassCreateInfo2* ci, const VkAllocationCallbacks* a, VkRenderPass* out) {
    VkResult r = device(dev)->CreateRenderPass2(dev, ci, a, out);
    if (r != VK_SUCCESS) return r;
    RenderPassInfo p;
    for (uint32_t i = 0; i < ci->attachmentCount; i++) {
        auto& x = ci->pAttachments[i];
        p.atts.push_back({x.format, x.samples, x.loadOp, x.storeOp, x.stencilLoadOp, x.finalLayout});
    }
    Lock l(mu());
    objs().passes[h64(*out)] = std::move(p);
    return r;
}

VkResult hk_CreateFramebuffer(VkDevice dev, const VkFramebufferCreateInfo* ci, const VkAllocationCallbacks* a, VkFramebuffer* out) {
    VkResult r = device(dev)->CreateFramebuffer(dev, ci, a, out);
    if (r != VK_SUCCESS) return r;
    FramebufferInfo f{{}, ci->width, ci->height};
    if (!(ci->flags & VK_FRAMEBUFFER_CREATE_IMAGELESS_BIT) && ci->pAttachments)
        f.views.assign(ci->pAttachments, ci->pAttachments + ci->attachmentCount);
    Lock l(mu());
    objs().fbs[h64(*out)] = std::move(f);
    return r;
}
