// Command recording while a capture is armed: passes, bindings, draws and dispatches per command buffer.
#include "capture.h"
#include <cstring>

using namespace gpu;
using Lock = std::lock_guard<std::mutex>;

namespace gpu {

std::unordered_map<uint64_t, CmdRec>& cmds() { static std::unordered_map<uint64_t, CmdRec> m; return m; }
std::atomic<bool>& recording() { static std::atomic<bool> on{false}; return on; }

CmdRec* rec(VkCommandBuffer cb) {
    auto it = cmds().find(h64(cb));
    return it == cmds().end() ? nullptr : &it->second;
}

std::string label_of(const CmdRec& r) {
    std::string s;
    for (auto& l : r.labels) s += (s.empty() ? "" : "/") + l;
    return s;
}

}  // namespace gpu

static bool is_depth(VkFormat f) {
    return f == VK_FORMAT_D16_UNORM || f == VK_FORMAT_D32_SFLOAT || f == VK_FORMAT_X8_D24_UNORM_PACK32 ||
           f == VK_FORMAT_D24_UNORM_S8_UINT || f == VK_FORMAT_D32_SFLOAT_S8_UINT || f == VK_FORMAT_S8_UINT || f == VK_FORMAT_D16_UNORM_S8_UINT;
}

static VkSampleCountFlagBits samples_of(VkImageView v) {
    auto vi = objs().views.find(h64(v));
    if (vi == objs().views.end()) return VK_SAMPLE_COUNT_1_BIT;
    auto ii = objs().images.find(h64(vi->second.image));
    return ii == objs().images.end() ? VK_SAMPLE_COUNT_1_BIT : ii->second.samples;
}

static VkFormat format_of(VkImageView v) {
    auto vi = objs().views.find(h64(v));
    return vi == objs().views.end() ? VK_FORMAT_UNDEFINED : vi->second.fmt;
}

static void open_pass(CmdRec& r, std::shared_ptr<PassRec> p) {
    p->label = label_of(r);
    dump_begin_pass(*p);
    r.pass = p;
    r.drawInPass = 0;
    r.ev.push_back({Event::Pass, "", p});
}

VkResult hk_BeginCommandBuffer(VkCommandBuffer cb, const VkCommandBufferBeginInfo* bi) {
    {
        Lock l(mu());
        uploads_begin(cb);
        if (recording()) cmds()[h64(cb)] = CmdRec{};
        else cmds().erase(h64(cb));
    }
    return device(cb)->BeginCommandBuffer(cb, bi);
}

static void begin_pass(VkCommandBuffer cb, const VkRenderPassBeginInfo* bi) {
    with(cb, [&](CmdRec& r) {
        auto p = std::make_shared<PassRec>();
        p->area = bi->renderArea;
        auto rp = objs().passes.find(h64(bi->renderPass));
        auto fb = objs().fbs.find(h64(bi->framebuffer));
        if (rp == objs().passes.end() || fb == objs().fbs.end()) return;
        std::vector<VkImageView> views = fb->second.views;
        for (auto n = (const VkBaseInStructure*)bi->pNext; n; n = n->pNext)
            if (n->sType == VK_STRUCTURE_TYPE_RENDER_PASS_ATTACHMENT_BEGIN_INFO) {
                auto ab = (const VkRenderPassAttachmentBeginInfo*)n;
                views.assign(ab->pAttachments, ab->pAttachments + ab->attachmentCount);
            }
        auto& atts = rp->second.atts;
        for (size_t i = 0; i < atts.size(); i++) {
            VkClearValue c{};
            if (i < bi->clearValueCount) c = bi->pClearValues[i];
            VkAttachmentLoadOp load = is_depth(atts[i].fmt) && atts[i].fmt == VK_FORMAT_S8_UINT ? atts[i].sload : atts[i].load;
            p->atts.push_back({i < views.size() ? views[i] : VK_NULL_HANDLE, atts[i].fmt, atts[i].samples, load, atts[i].store,
                               atts[i].final, c, is_depth(atts[i].fmt)});
        }
        open_pass(r, p);
    });
}

void hk_CmdBeginRenderPass(VkCommandBuffer cb, const VkRenderPassBeginInfo* bi, VkSubpassContents c) {
    device(cb)->CmdBeginRenderPass(cb, bi, c);
    begin_pass(cb, bi);
}

void hk_CmdBeginRenderPass2(VkCommandBuffer cb, const VkRenderPassBeginInfo* bi, const VkSubpassBeginInfo* sb) {
    device(cb)->CmdBeginRenderPass2(cb, bi, sb);
    begin_pass(cb, bi);
}

void hk_CmdNextSubpass(VkCommandBuffer cb, VkSubpassContents c) {
    with(cb, [&](CmdRec& r) { r.ev.push_back({Event::Label, "subpass"}); });
    device(cb)->CmdNextSubpass(cb, c);
}

void hk_CmdNextSubpass2(VkCommandBuffer cb, const VkSubpassBeginInfo* b, const VkSubpassEndInfo* e) {
    with(cb, [&](CmdRec& r) { r.ev.push_back({Event::Label, "subpass"}); });
    device(cb)->CmdNextSubpass2(cb, b, e);
}

static void end_pass(VkCommandBuffer cb) {
    with(cb, [&](CmdRec& r) {
        if (!r.pass) return;
        dump_end_pass(device(cb), cb, r);
        r.ev.push_back({Event::End, "", r.pass});
        r.pass = nullptr;
    });
}

void hk_CmdEndRenderPass(VkCommandBuffer cb) { device(cb)->CmdEndRenderPass(cb); end_pass(cb); }
void hk_CmdEndRenderPass2(VkCommandBuffer cb, const VkSubpassEndInfo* e) { device(cb)->CmdEndRenderPass2(cb, e); end_pass(cb); }

void hk_CmdBeginRendering(VkCommandBuffer cb, const VkRenderingInfo* ri) {
    device(cb)->CmdBeginRendering(cb, ri);
    with(cb, [&](CmdRec& r) {
        auto p = std::make_shared<PassRec>();
        p->dynamic = true;
        p->area = ri->renderArea;
        auto add = [&](const VkRenderingAttachmentInfo* a, bool depth) {
            if (!a || !a->imageView) return;
            p->atts.push_back({a->imageView, format_of(a->imageView), samples_of(a->imageView), a->loadOp, a->storeOp, a->imageLayout, a->clearValue, depth});
        };
        for (uint32_t i = 0; i < ri->colorAttachmentCount; i++) add(&ri->pColorAttachments[i], false);
        add(ri->pDepthAttachment, true);
        if (ri->pStencilAttachment && (!ri->pDepthAttachment || ri->pStencilAttachment->imageView != ri->pDepthAttachment->imageView))
            add(ri->pStencilAttachment, true);
        if (ri->flags & VK_RENDERING_RESUMING_BIT) p->label += "(resumed)";
        open_pass(r, p);
    });
}

void hk_CmdEndRendering(VkCommandBuffer cb) { device(cb)->CmdEndRendering(cb); end_pass(cb); }

void hk_CmdBindPipeline(VkCommandBuffer cb, VkPipelineBindPoint bp, VkPipeline p) {
    with(cb, [&](CmdRec& r) { (bp == VK_PIPELINE_BIND_POINT_COMPUTE ? r.st.comp : r.st.gfx).pipe = p; });
    device(cb)->CmdBindPipeline(cb, bp, p);
}

static uint32_t dynamic_count(VkDescriptorSetLayout l) {
    auto it = objs().setLayouts.find(h64(l));
    uint32_t n = 0;
    if (it != objs().setLayouts.end())
        for (auto& b : it->second.bindings)
            if (b.type == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER_DYNAMIC || b.type == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER_DYNAMIC) n += b.count;
    return n;
}

static Bound& slot(CmdRec& r, VkPipelineBindPoint bp, uint32_t set) {
    auto& v = (bp == VK_PIPELINE_BIND_POINT_COMPUTE ? r.st.comp : r.st.gfx).sets;
    if (v.size() <= set) v.resize(set + 1);
    return v[set];
}

void hk_CmdBindDescriptorSets(VkCommandBuffer cb, VkPipelineBindPoint bp, VkPipelineLayout layout, uint32_t first, uint32_t n,
                              const VkDescriptorSet* s, uint32_t nd, const uint32_t* dyn) {
    with(cb, [&](CmdRec& r) {
        auto pl = objs().pipeLayouts.find(h64(layout));
        uint32_t used = 0;
        for (uint32_t i = 0; i < n; i++) {
            Bound& b = slot(r, bp, first + i);
            b = Bound{s[i], nullptr, {}};
            if (pl == objs().pipeLayouts.end() || first + i >= pl->second.sets.size()) continue;
            for (uint32_t k = dynamic_count(pl->second.sets[first + i]); k && used < nd; k--) b.offs.push_back(dyn[used++]);
        }
    });
    device(cb)->CmdBindDescriptorSets(cb, bp, layout, first, n, s, nd, dyn);
}

static std::shared_ptr<SetData> pushed(CmdRec& r, VkPipelineBindPoint bp, VkPipelineLayout layout, uint32_t set) {
    Bound& b = slot(r, bp, set);
    auto pl = objs().pipeLayouts.find(h64(layout));
    VkDescriptorSetLayout sl = pl != objs().pipeLayouts.end() && set < pl->second.sets.size() ? pl->second.sets[set] : VK_NULL_HANDLE;
    auto s = b.pushed ? std::make_shared<SetData>(*b.pushed) : new_set(sl);
    b = Bound{VK_NULL_HANDLE, s, {}};
    return s;
}

void hk_CmdPushDescriptorSet(VkCommandBuffer cb, VkPipelineBindPoint bp, VkPipelineLayout layout, uint32_t set, uint32_t n, const VkWriteDescriptorSet* w) {
    with(cb, [&](CmdRec& r) {
        auto s = pushed(r, bp, layout, set);
        for (uint32_t i = 0; i < n; i++) apply_write(*s, w[i]);
    });
    device(cb)->CmdPushDescriptorSet(cb, bp, layout, set, n, w);
}

void hk_CmdPushDescriptorSetWithTemplate(VkCommandBuffer cb, VkDescriptorUpdateTemplate t, VkPipelineLayout layout, uint32_t set, const void* data) {
    // The template's bind point isn't kept; push templates in these engines are graphics.
    with(cb, [&](CmdRec& r) { apply_template(*pushed(r, VK_PIPELINE_BIND_POINT_GRAPHICS, layout, set), h64(t), data); });
    device(cb)->CmdPushDescriptorSetWithTemplate(cb, t, layout, set, data);
}

void hk_CmdPushConstants(VkCommandBuffer cb, VkPipelineLayout layout, VkShaderStageFlags st, uint32_t off, uint32_t size, const void* v) {
    with(cb, [&](CmdRec& r) {
        if (r.st.pc.size() < off + size) r.st.pc.resize(off + size);
        memcpy(r.st.pc.data() + off, v, size);
    });
    device(cb)->CmdPushConstants(cb, layout, st, off, size, v);
}

void hk_CmdBindVertexBuffers(VkCommandBuffer cb, uint32_t first, uint32_t n, const VkBuffer* b, const VkDeviceSize* o) {
    with(cb, [&](CmdRec& r) {
        if (r.st.vbs.size() < first + n) r.st.vbs.resize(first + n);
        for (uint32_t i = 0; i < n; i++) r.st.vbs[first + i] = {b[i], o[i], 0};
    });
    device(cb)->CmdBindVertexBuffers(cb, first, n, b, o);
}

void hk_CmdBindVertexBuffers2(VkCommandBuffer cb, uint32_t first, uint32_t n, const VkBuffer* b, const VkDeviceSize* o,
                              const VkDeviceSize* sizes, const VkDeviceSize* strides) {
    with(cb, [&](CmdRec& r) {
        if (r.st.vbs.size() < first + n) r.st.vbs.resize(first + n);
        for (uint32_t i = 0; i < n; i++) r.st.vbs[first + i] = {b[i], o[i], strides ? strides[i] : 0};
    });
    device(cb)->CmdBindVertexBuffers2(cb, first, n, b, o, sizes, strides);
}

void hk_CmdBindIndexBuffer(VkCommandBuffer cb, VkBuffer b, VkDeviceSize o, VkIndexType t) {
    with(cb, [&](CmdRec& r) { r.st.ib = b; r.st.ibOff = o; r.st.ibType = t; });
    device(cb)->CmdBindIndexBuffer(cb, b, o, t);
}

void hk_CmdExecuteCommands(VkCommandBuffer cb, uint32_t n, const VkCommandBuffer* s) {
    with(cb, [&](CmdRec& r) { r.ev.push_back({Event::Exec, "", nullptr, nullptr, {s, s + n}}); });
    device(cb)->CmdExecuteCommands(cb, n, s);
}

void hk_CmdBeginDebugUtilsLabelEXT(VkCommandBuffer cb, const VkDebugUtilsLabelEXT* l) {
    with(cb, [&](CmdRec& r) { r.labels.push_back(l->pLabelName ? l->pLabelName : ""); r.ev.push_back({Event::Label, "+" + r.labels.back()}); });
    device(cb)->CmdBeginDebugUtilsLabelEXT(cb, l);
}

void hk_CmdEndDebugUtilsLabelEXT(VkCommandBuffer cb) {
    with(cb, [&](CmdRec& r) { if (!r.labels.empty()) r.labels.pop_back(); r.ev.push_back({Event::Label, "-"}); });
    device(cb)->CmdEndDebugUtilsLabelEXT(cb);
}

void hk_CmdInsertDebugUtilsLabelEXT(VkCommandBuffer cb, const VkDebugUtilsLabelEXT* l) {
    with(cb, [&](CmdRec& r) { r.ev.push_back({Event::Label, l->pLabelName ? l->pLabelName : ""}); });
    device(cb)->CmdInsertDebugUtilsLabelEXT(cb, l);
}
