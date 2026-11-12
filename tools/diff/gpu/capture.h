// Pipelines, descriptor sets and per-command-buffer recordings, shared by the capture's sources.
#pragma once
#include "layer.h"
#include <cstdio>
#include <set>

namespace gpu {

// Graphics pipeline library parts (VkGraphicsPipelineLibraryFlagBitsEXT order).
enum Part : uint32_t { P_VI = 1, P_PRE = 2, P_FS = 4, P_OUT = 8, P_ALL = 15 };
struct Stage { VkShaderStageFlagBits stage; ShaderInfo sh; std::string entry; };
struct Blend { VkBool32 en; VkBlendFactor sc, dc, sa, da; VkBlendOp co, ao; VkColorComponentFlags mask; };
struct PipeInfo {
    bool compute = false;
    uint32_t parts = 0;
    std::vector<Stage> stages;
    std::vector<VkVertexInputBindingDescription> vb;
    std::vector<VkVertexInputAttributeDescription> va;
    VkPrimitiveTopology topo = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;
    VkBool32 restart = 0;
    VkPipelineRasterizationStateCreateInfo rs{};
    VkSampleCountFlagBits samples = VK_SAMPLE_COUNT_1_BIT;
    VkBool32 a2c = 0;
    VkPipelineDepthStencilStateCreateInfo ds{};
    std::vector<Blend> blend;
    float bc[4]{};
    std::vector<VkDynamicState> dyn;
    std::vector<VkFormat> colorFmts;
    VkFormat depthFmt = VK_FORMAT_UNDEFINED;
    VkPipelineLayout layout = VK_NULL_HANDLE;
    bool has_dyn(VkDynamicState s) const { return std::find(dyn.begin(), dyn.end(), s) != dyn.end(); }
};
std::unordered_map<uint64_t, std::shared_ptr<PipeInfo>>& pipes();

struct Desc { VkDescriptorType type; VkImageView view; VkSampler sampler; VkImageLayout layout; VkBuffer buf; VkDeviceSize off, range; };
struct SetData { VkDescriptorSetLayout layout = VK_NULL_HANDLE; std::map<uint32_t, std::vector<Desc>> b; };
std::unordered_map<uint64_t, std::shared_ptr<SetData>>& sets();
std::shared_ptr<SetData> new_set(VkDescriptorSetLayout layout);
void write_desc(SetData& s, uint32_t binding, uint32_t elem, const Desc& d);
void apply_template(SetData& s, uint64_t tmpl, const void* data);
void apply_write(SetData& s, const VkWriteDescriptorSet& w);

enum DynBit : uint32_t {
    D_VIEWPORT = 1 << 0, D_SCISSOR = 1 << 1, D_CULL = 1 << 2, D_FRONT = 1 << 3, D_TOPO = 1 << 4, D_DTEST = 1 << 5,
    D_DWRITE = 1 << 6, D_DCMP = 1 << 7, D_BIASEN = 1 << 8, D_BIAS = 1 << 9, D_STEST = 1 << 10, D_SREF = 1 << 11,
    D_BC = 1 << 12, D_BLENDEN = 1 << 13, D_MASK = 1 << 14, D_EQ = 1 << 15, D_POLY = 1 << 16,
};
struct Dyn {
    uint32_t set = 0;
    VkViewport vp{};
    VkRect2D sc{};
    VkCullModeFlags cull = 0;
    VkFrontFace front{};
    VkPrimitiveTopology topo{};
    VkBool32 dTest = 0, dWrite = 0, biasEn = 0, sTest = 0;
    VkCompareOp dCmp{};
    float bias[3]{};
    uint32_t sref = 0;
    float bc[4]{};
    VkPolygonMode poly{};
    std::vector<VkBool32> blendEn;
    std::vector<VkColorComponentFlags> mask;
    std::vector<VkColorBlendEquationEXT> eq;
};
struct Bound { VkDescriptorSet set = VK_NULL_HANDLE; std::shared_ptr<SetData> pushed; std::vector<uint32_t> offs; };
struct BindPoint { VkPipeline pipe = VK_NULL_HANDLE; std::vector<Bound> sets; };
struct VB { VkBuffer buf; VkDeviceSize off; VkDeviceSize stride; };
struct State {
    BindPoint gfx, comp;
    std::vector<uint8_t> pc;
    std::vector<VB> vbs;
    VkBuffer ib = VK_NULL_HANDLE;
    VkDeviceSize ibOff = 0;
    VkIndexType ibType = VK_INDEX_TYPE_UINT16;
    Dyn dyn;
};

struct Att { VkImageView view; VkFormat fmt; VkSampleCountFlagBits samples; VkAttachmentLoadOp load; VkAttachmentStoreOp store; VkImageLayout layout; VkClearValue clear; bool depth; };
// sel: attachment signature. dumped: copies injected (dump.cpp); chosen: the dump target in submit order.
struct PassRec { bool dynamic = false; std::vector<Att> atts; VkRect2D area{}; std::string sel, label; bool dumped = false, chosen = false; int dumpDraw = -1; };
struct DrawRec {
    std::string cmd;
    uint32_t n = 0, inst = 1, first = 0, finst = 0;
    int32_t vofs = 0;
    bool indexed = false, skipped = false;
    VkBuffer indirect = VK_NULL_HANDLE;
    VkDeviceSize ioff = 0;
    uint32_t icount = 1, istride = 0;
    uint32_t group[3]{};
    std::vector<VkMultiDrawIndexedInfoEXT> multi;
    State st;
    std::string label;
};
struct DumpReq { std::shared_ptr<PassRec> pass; uint32_t att, draw; VkBuffer buf; VkDeviceMemory mem; VkFormat fmt; uint32_t w, h; };
struct Event {
    enum Kind { Pass, End, Draw, Dispatch, Xfer, Clear, Label, Exec } k;
    std::string text;
    std::shared_ptr<PassRec> pass;
    std::shared_ptr<DrawRec> draw;
    std::vector<VkCommandBuffer> exec;
    uint64_t dst = 0;
};
struct CmdRec {
    std::vector<Event> ev;
    State st;
    std::shared_ptr<PassRec> pass;
    uint32_t drawInPass = 0;
    std::vector<std::string> labels;
    std::vector<DumpReq> dumps;
};
std::unordered_map<uint64_t, CmdRec>& cmds();
std::atomic<bool>& recording();
// The recording for cb, or null when it began while recording was off (mu held).
CmdRec* rec(VkCommandBuffer cb);
std::string label_of(const CmdRec& r);
// Runs f(rec) under the lock when cb is being recorded.
template <class F> void with(VkCommandBuffer cb, F f) {
    if (!recording().load(std::memory_order_relaxed)) return;
    std::lock_guard<std::mutex> l(mu());
    if (CmdRec* r = rec(cb)) f(*r);
}

// Render-target dumps (dump.cpp).
void dump_new_frame();
// Called for each pass in submit order: learns the target in lead frames, marks it when capturing.
void dump_submit_order(PassRec& p, bool capturing);
void dump_begin_pass(PassRec& p);
// Writes and frees r's dumps; with out null, only frees them.
void dump_end_pass(Device* d, VkCommandBuffer cb, CmdRec& r);
void dump_write(Device* d, CmdRec& r, FILE* out, const std::string& dir);

// Frame output (emit.cpp).
struct Frame {
    FILE* f = nullptr;
    std::string dir;
    int pass = -1, draws = 0, unrecorded = 0;
    std::unordered_map<uint64_t, int> pipeIds;
    std::set<uint64_t> blobs;
    std::set<std::pair<uint64_t, uint32_t>> bindless;
};
void emit_cmdbuf(Device* d, Frame& fr, VkCommandBuffer cb);

// Enum names (names.cpp), vulkan.hpp's spelling.
#define UB_NAMED(X) X(Format) X(CompareOp) X(BlendFactor) X(BlendOp) X(PrimitiveTopology) X(PolygonMode) X(FrontFace) \
    X(DescriptorType) X(AttachmentLoadOp) X(AttachmentStoreOp) X(ImageLayout) X(Filter) X(SamplerMipmapMode) \
    X(SamplerAddressMode) X(ImageViewType) X(DynamicState) X(IndexType) X(StencilOp) X(ShaderStageFlagBits) X(BorderColor)
#define UB_NM(T) std::string nm(Vk##T v);
UB_NAMED(UB_NM)
#undef UB_NM
std::string cull_name(VkCullModeFlags c);

}  // namespace gpu
