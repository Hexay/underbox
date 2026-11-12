// Descriptor set contents: writes, copies and update templates, resolved per draw at submit.
#include "capture.h"

using namespace gpu;
using Lock = std::lock_guard<std::mutex>;

namespace gpu {

std::unordered_map<uint64_t, std::shared_ptr<SetData>>& sets() { static std::unordered_map<uint64_t, std::shared_ptr<SetData>> m; return m; }
static std::unordered_map<uint64_t, std::vector<VkDescriptorUpdateTemplateEntry>>& templates() {
    static std::unordered_map<uint64_t, std::vector<VkDescriptorUpdateTemplateEntry>> m;
    return m;
}

std::shared_ptr<SetData> new_set(VkDescriptorSetLayout layout) {
    auto s = std::make_shared<SetData>();
    s->layout = layout;
    auto it = objs().setLayouts.find(h64(layout));
    if (it != objs().setLayouts.end())
        for (auto& b : it->second.bindings) s->b[b.binding].assign(b.count, Desc{b.type, VK_NULL_HANDLE, VK_NULL_HANDLE, VK_IMAGE_LAYOUT_UNDEFINED, VK_NULL_HANDLE, 0, 0});
    return s;
}

// Writes past a binding's end continue in the next binding, as the spec's consecutive-binding rule says.
void write_desc(SetData& s, uint32_t binding, uint32_t elem, const Desc& d) {
    auto it = s.b.find(binding);
    while (it != s.b.end() && elem >= it->second.size()) {
        elem -= (uint32_t)it->second.size();
        ++it;
    }
    if (it == s.b.end()) return;
    Desc& x = it->second[elem];
    VkDescriptorType keep = x.type;
    x = d;
    if (d.type == VK_DESCRIPTOR_TYPE_SAMPLER && keep == VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER) x.type = keep;
}

static Desc from_image(VkDescriptorType t, const VkDescriptorImageInfo& i) { return {t, i.imageView, i.sampler, i.imageLayout, VK_NULL_HANDLE, 0, 0}; }
static Desc from_buffer(VkDescriptorType t, const VkDescriptorBufferInfo& b) { return {t, VK_NULL_HANDLE, VK_NULL_HANDLE, VK_IMAGE_LAYOUT_UNDEFINED, b.buffer, b.offset, b.range}; }

static bool is_image(VkDescriptorType t) {
    return t == VK_DESCRIPTOR_TYPE_SAMPLER || t == VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER || t == VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE ||
           t == VK_DESCRIPTOR_TYPE_STORAGE_IMAGE || t == VK_DESCRIPTOR_TYPE_INPUT_ATTACHMENT;
}
static bool is_buffer(VkDescriptorType t) {
    return t == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER || t == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER ||
           t == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER_DYNAMIC || t == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER_DYNAMIC;
}

void apply_template(SetData& s, uint64_t tmpl, const void* data) {
    auto it = templates().find(tmpl);
    if (it == templates().end()) return;
    for (auto& e : it->second) {
        for (uint32_t i = 0; i < e.descriptorCount; i++) {
            auto p = (const uint8_t*)data + e.offset + i * e.stride;
            if (is_image(e.descriptorType)) write_desc(s, e.dstBinding, e.dstArrayElement + i, from_image(e.descriptorType, *(const VkDescriptorImageInfo*)p));
            else if (is_buffer(e.descriptorType)) write_desc(s, e.dstBinding, e.dstArrayElement + i, from_buffer(e.descriptorType, *(const VkDescriptorBufferInfo*)p));
            else write_desc(s, e.dstBinding, e.dstArrayElement + i, {e.descriptorType, VK_NULL_HANDLE, VK_NULL_HANDLE, VK_IMAGE_LAYOUT_UNDEFINED, VK_NULL_HANDLE, 0, 0});
        }
    }
}

void apply_write(SetData& s, const VkWriteDescriptorSet& w) {
    for (uint32_t i = 0; i < w.descriptorCount; i++) {
        Desc d{w.descriptorType, VK_NULL_HANDLE, VK_NULL_HANDLE, VK_IMAGE_LAYOUT_UNDEFINED, VK_NULL_HANDLE, 0, 0};
        if (is_image(w.descriptorType) && w.pImageInfo) d = from_image(w.descriptorType, w.pImageInfo[i]);
        else if (is_buffer(w.descriptorType) && w.pBufferInfo) d = from_buffer(w.descriptorType, w.pBufferInfo[i]);
        write_desc(s, w.dstBinding, w.dstArrayElement + i, d);
    }
}

}  // namespace gpu

VkResult hk_AllocateDescriptorSets(VkDevice dev, const VkDescriptorSetAllocateInfo* ai, VkDescriptorSet* out) {
    VkResult r = device(dev)->AllocateDescriptorSets(dev, ai, out);
    if (r != VK_SUCCESS) return r;
    Lock l(mu());
    for (uint32_t i = 0; i < ai->descriptorSetCount; i++) sets()[h64(out[i])] = new_set(ai->pSetLayouts[i]);
    return r;
}

void hk_UpdateDescriptorSets(VkDevice dev, uint32_t nw, const VkWriteDescriptorSet* w, uint32_t nc, const VkCopyDescriptorSet* c) {
    {
        Lock l(mu());
        for (uint32_t i = 0; i < nw; i++) {
            auto it = sets().find(h64(w[i].dstSet));
            if (it != sets().end()) apply_write(*it->second, w[i]);
        }
        for (uint32_t i = 0; i < nc; i++) {
            auto src = sets().find(h64(c[i].srcSet)), dst = sets().find(h64(c[i].dstSet));
            if (src == sets().end() || dst == sets().end()) continue;
            auto sb = src->second->b.find(c[i].srcBinding);
            for (uint32_t k = 0; sb != src->second->b.end() && k < c[i].descriptorCount; k++) {
                uint32_t e = c[i].srcArrayElement + k;
                if (e < sb->second.size()) write_desc(*dst->second, c[i].dstBinding, c[i].dstArrayElement + k, sb->second[e]);
            }
        }
    }
    device(dev)->UpdateDescriptorSets(dev, nw, w, nc, c);
}

VkResult hk_CreateDescriptorUpdateTemplate(VkDevice dev, const VkDescriptorUpdateTemplateCreateInfo* ci, const VkAllocationCallbacks* a,
                                           VkDescriptorUpdateTemplate* out) {
    VkResult r = device(dev)->CreateDescriptorUpdateTemplate(dev, ci, a, out);
    if (r != VK_SUCCESS) return r;
    Lock l(mu());
    templates()[h64(*out)].assign(ci->pDescriptorUpdateEntries, ci->pDescriptorUpdateEntries + ci->descriptorUpdateEntryCount);
    return r;
}

void hk_UpdateDescriptorSetWithTemplate(VkDevice dev, VkDescriptorSet set, VkDescriptorUpdateTemplate t, const void* data) {
    {
        Lock l(mu());
        auto it = sets().find(h64(set));
        if (it != sets().end()) apply_template(*it->second, h64(t), data);
    }
    device(dev)->UpdateDescriptorSetWithTemplate(dev, set, t, data);
}
