// underbox GPU capture layer: records one presented frame's Vulkan work as JSON lines.
// See docs/differential-gpu.md. Built by tools/diff/gpu-layer.sh.
#pragma once
#include <vulkan/vulkan.h>
#include <vulkan/vk_layer.h>
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <type_traits>
#include <unordered_map>
#include <vector>

// Next-layer functions a device needs. Each resolves by core name, then the KHR and EXT aliases.
#define UB_NEXT(X) X(DestroyDevice) X(QueueSubmit) X(QueueSubmit2) X(QueuePresentKHR) X(QueueWaitIdle) \
    X(CreateImage) X(DestroyImage) X(CreateImageView) X(CreateSampler) X(CreateBuffer) X(DestroyBuffer) \
    X(BindBufferMemory) X(BindBufferMemory2) X(AllocateMemory) X(FreeMemory) X(MapMemory) X(UnmapMemory) \
    X(MapMemory2) X(UnmapMemory2) X(GetBufferMemoryRequirements) X(CreateSwapchainKHR) X(GetSwapchainImagesKHR) \
    X(SetDebugUtilsObjectNameEXT) X(CreateShaderModule) X(CreateGraphicsPipelines) X(CreateComputePipelines) \
    X(CreatePipelineLayout) X(CreateDescriptorSetLayout) X(CreateRenderPass) X(CreateRenderPass2) \
    X(CreateFramebuffer) X(AllocateDescriptorSets) X(UpdateDescriptorSets) X(CreateDescriptorUpdateTemplate) \
    X(UpdateDescriptorSetWithTemplate) X(BeginCommandBuffer) X(CmdBeginRenderPass) X(CmdBeginRenderPass2) \
    X(CmdNextSubpass) X(CmdNextSubpass2) X(CmdEndRenderPass) X(CmdEndRenderPass2) X(CmdBeginRendering) \
    X(CmdEndRendering) X(CmdBindPipeline) X(CmdBindDescriptorSets) X(CmdPushDescriptorSet) \
    X(CmdPushDescriptorSetWithTemplate) X(CmdPushConstants) X(CmdBindVertexBuffers) X(CmdBindVertexBuffers2) \
    X(CmdBindIndexBuffer) X(CmdDraw) X(CmdDrawIndexed) X(CmdDrawIndirect) X(CmdDrawIndexedIndirect) \
    X(CmdDrawIndirectCount) X(CmdDrawIndexedIndirectCount) X(CmdDrawMultiEXT) X(CmdDrawMultiIndexedEXT) \
    X(CmdDispatch) X(CmdDispatchIndirect) X(CmdSetViewport) X(CmdSetScissor) X(CmdSetCullMode) \
    X(CmdSetFrontFace) X(CmdSetPrimitiveTopology) X(CmdSetDepthTestEnable) X(CmdSetDepthWriteEnable) \
    X(CmdSetDepthCompareOp) X(CmdSetDepthBiasEnable) X(CmdSetDepthBias) X(CmdSetStencilTestEnable) \
    X(CmdSetStencilReference) X(CmdSetStencilOp) X(CmdSetStencilCompareMask) X(CmdSetStencilWriteMask) \
    X(CmdSetBlendConstants) X(CmdSetColorBlendEnableEXT) X(CmdSetColorWriteMaskEXT) \
    X(CmdSetColorBlendEquationEXT) X(CmdSetPolygonModeEXT) X(CmdClearAttachments) X(CmdClearColorImage) \
    X(CmdClearDepthStencilImage) X(CmdCopyBuffer) X(CmdCopyBufferToImage) X(CmdCopyImage) X(CmdCopyImageToBuffer) \
    X(CmdBlitImage) X(CmdResolveImage) X(CmdUpdateBuffer) X(CmdFillBuffer) X(CmdCopyBuffer2) \
    X(CmdCopyBufferToImage2) X(CmdCopyImage2) X(CmdCopyImageToBuffer2) X(CmdBlitImage2) X(CmdExecuteCommands) \
    X(CmdBeginDebugUtilsLabelEXT) X(CmdEndDebugUtilsLabelEXT) X(CmdInsertDebugUtilsLabelEXT) X(CmdPipelineBarrier)

// Hooked device functions: everything in UB_NEXT that the app calls, minus helpers only we call.
#define UB_HOOKS(X) X(DestroyDevice) X(QueueSubmit) X(QueueSubmit2) X(QueuePresentKHR) X(CreateImage) \
    X(DestroyImage) X(CreateImageView) X(CreateSampler) X(CreateBuffer) X(DestroyBuffer) X(BindBufferMemory) \
    X(BindBufferMemory2) X(AllocateMemory) X(FreeMemory) X(MapMemory) X(UnmapMemory) X(MapMemory2) X(UnmapMemory2) \
    X(CreateSwapchainKHR) X(GetSwapchainImagesKHR) X(SetDebugUtilsObjectNameEXT) X(CreateShaderModule) \
    X(CreateGraphicsPipelines) X(CreateComputePipelines) X(CreatePipelineLayout) X(CreateDescriptorSetLayout) \
    X(CreateRenderPass) X(CreateRenderPass2) X(CreateFramebuffer) X(AllocateDescriptorSets) X(UpdateDescriptorSets) \
    X(CreateDescriptorUpdateTemplate) X(UpdateDescriptorSetWithTemplate) X(BeginCommandBuffer) X(CmdBeginRenderPass) \
    X(CmdBeginRenderPass2) X(CmdNextSubpass) X(CmdNextSubpass2) X(CmdEndRenderPass) X(CmdEndRenderPass2) \
    X(CmdBeginRendering) X(CmdEndRendering) X(CmdBindPipeline) X(CmdBindDescriptorSets) X(CmdPushDescriptorSet) \
    X(CmdPushDescriptorSetWithTemplate) X(CmdPushConstants) X(CmdBindVertexBuffers) X(CmdBindVertexBuffers2) \
    X(CmdBindIndexBuffer) X(CmdDraw) X(CmdDrawIndexed) X(CmdDrawIndirect) X(CmdDrawIndexedIndirect) \
    X(CmdDrawIndirectCount) X(CmdDrawIndexedIndirectCount) X(CmdDrawMultiEXT) X(CmdDrawMultiIndexedEXT) \
    X(CmdDispatch) X(CmdDispatchIndirect) X(CmdSetViewport) X(CmdSetScissor) X(CmdSetCullMode) X(CmdSetFrontFace) \
    X(CmdSetPrimitiveTopology) X(CmdSetDepthTestEnable) X(CmdSetDepthWriteEnable) X(CmdSetDepthCompareOp) \
    X(CmdSetDepthBiasEnable) X(CmdSetDepthBias) X(CmdSetStencilTestEnable) X(CmdSetStencilReference) \
    X(CmdSetStencilOp) X(CmdSetStencilCompareMask) X(CmdSetStencilWriteMask) X(CmdSetBlendConstants) X(CmdSetColorBlendEnableEXT) X(CmdSetColorWriteMaskEXT) X(CmdSetColorBlendEquationEXT) \
    X(CmdSetPolygonModeEXT) X(CmdClearAttachments) X(CmdClearColorImage) X(CmdClearDepthStencilImage) \
    X(CmdCopyBuffer) X(CmdCopyBufferToImage) X(CmdCopyImage) X(CmdCopyImageToBuffer) X(CmdBlitImage) \
    X(CmdResolveImage) X(CmdUpdateBuffer) X(CmdFillBuffer) X(CmdCopyBuffer2) X(CmdCopyBufferToImage2) \
    X(CmdCopyImage2) X(CmdCopyImageToBuffer2) X(CmdBlitImage2) X(CmdExecuteCommands) \
    X(CmdBeginDebugUtilsLabelEXT) X(CmdEndDebugUtilsLabelEXT) X(CmdInsertDebugUtilsLabelEXT)

#define UB_DECLARE(n) extern std::remove_pointer_t<PFN_vk##n> hk_##n;
UB_HOOKS(UB_DECLARE)
#undef UB_DECLARE

namespace gpu {

struct Device {
    void* key = nullptr;
    VkDevice dev = VK_NULL_HANDLE;
    PFN_vkGetDeviceProcAddr gdpa = nullptr;
    VkPhysicalDeviceMemoryProperties mem{};
#define UB_MEMBER(n) PFN_vk##n n = nullptr;
    UB_NEXT(UB_MEMBER)
#undef UB_MEMBER
};

inline void* key(const void* dispatchable) { return *(void* const*)dispatchable; }
Device* device(const void* dispatchable);
uint64_t fnv(const void* data, size_t n, uint64_t h = 1469598103934665603ull);
std::string hex(uint64_t v);

// Everything below is guarded by mu().
std::mutex& mu();

// Content of uploaded mips (uploads.cpp): per mip an order-independent sum of row hashes; sample holds the
// smallest uploaded mip's layer-0 bytes when they are few.
struct ImageContent { std::map<uint32_t, uint64_t> mipHash; uint32_t sampleMip = UINT32_MAX; std::vector<uint8_t> sample; };
struct ImageInfo {
    VkImageType type; VkFormat fmt; VkExtent3D ext; uint32_t mips, layers; VkSampleCountFlagBits samples; bool swapchain = false;
    ImageContent content{};
};
struct ViewInfo { VkImage image; VkFormat fmt; VkImageViewType type; VkImageSubresourceRange range; };
struct BufferInfo { VkDeviceSize size; VkBufferUsageFlags usage; VkDeviceMemory mem = VK_NULL_HANDLE; VkDeviceSize memOffset = 0; };
struct MemInfo { VkDeviceSize size; uint32_t type; void* mapped = nullptr; VkDeviceSize mapOffset = 0; };
struct ShaderInfo { uint64_t hash; size_t size; std::string src; };
struct LayoutBinding { uint32_t binding; VkDescriptorType type; uint32_t count; VkShaderStageFlags stages; };
struct SetLayoutInfo { std::vector<LayoutBinding> bindings; };
struct PipeLayoutInfo { std::vector<VkDescriptorSetLayout> sets; std::vector<VkPushConstantRange> pc; };
struct AttDesc { VkFormat fmt; VkSampleCountFlagBits samples; VkAttachmentLoadOp load; VkAttachmentStoreOp store; VkAttachmentLoadOp sload; VkImageLayout final; };
struct RenderPassInfo { std::vector<AttDesc> atts; };
struct FramebufferInfo { std::vector<VkImageView> views; uint32_t w, h; };

struct Objects {
    std::unordered_map<uint64_t, ImageInfo> images;
    std::unordered_map<uint64_t, ViewInfo> views;
    std::unordered_map<uint64_t, VkSamplerCreateInfo> samplers;
    std::unordered_map<uint64_t, BufferInfo> buffers;
    std::unordered_map<uint64_t, MemInfo> mems;
    std::unordered_map<uint64_t, ShaderInfo> shaders;
    std::unordered_map<uint64_t, SetLayoutInfo> setLayouts;
    std::unordered_map<uint64_t, PipeLayoutInfo> pipeLayouts;
    std::unordered_map<uint64_t, RenderPassInfo> passes;
    std::unordered_map<uint64_t, FramebufferInfo> fbs;
    std::unordered_map<uint64_t, std::string> names;
    std::unordered_map<uint64_t, VkSwapchainCreateInfoKHR> swapchains;
};
Objects& objs();
template <class T> uint64_t h64(T h) { return (uint64_t)h; }

ShaderInfo shader_info(const uint32_t* code, size_t bytes);
// Bytes of buffer [off, off+n) as the device sees them after a wait-idle; empty if unreadable.
std::vector<uint8_t> read_buffer(Device* d, VkBuffer b, VkDeviceSize off, VkDeviceSize n);
bool host_visible(Device* d, uint32_t type);
uint32_t texel_bytes(VkFormat f);

}  // namespace gpu
