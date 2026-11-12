// JSON descriptions of images, views, samplers, pipelines and the effective draw state.
#include "json.h"

namespace gpu {

static std::string name_of(uint64_t h) {
    auto it = objs().names.find(h);
    return it == objs().names.end() ? "" : it->second;
}

std::string image_obj(uint64_t image) {
    auto it = objs().images.find(image);
    if (it == objs().images.end()) return "null";
    auto& i = it->second;
    Obj o;
    o.str("fmt", nm(i.fmt)).n("w", i.ext.width).n("h", i.ext.height).n("d", i.ext.depth).n("m", i.mips).n("l", i.layers).n("s", i.samples);
    if (i.swapchain) o.n("swapchain", 1);
    std::string n = name_of(image);
    if (!n.empty()) o.str("name", n);
    // Content: the hash of the finest uploaded mip (hmip when that isn't mip 0) and the smallest mip's bytes.
    auto& c = i.content;
    if (!c.mipHash.empty()) {
        o.str("hash", hex(c.mipHash.begin()->second));
        if (c.mipHash.begin()->first) o.n("hmip", c.mipHash.begin()->first);
        o.n("upmips", (double)c.mipHash.size());
    }
    if (!c.sample.empty()) o.n("smip", c.sampleMip).str("sample", hexbytes(c.sample.data(), c.sample.size()));
    return o.done();
}

std::string view_obj(VkImageView v) {
    auto it = objs().views.find(h64(v));
    if (it == objs().views.end()) return "null";
    auto& x = it->second;
    Obj o;
    o.raw("img", image_obj(h64(x.image))).str("type", nm(x.type)).n("mip", x.range.baseMipLevel).n("mips", x.range.levelCount);
    if (x.range.baseArrayLayer) o.n("layer", x.range.baseArrayLayer);
    auto img = objs().images.find(h64(x.image));
    if (img != objs().images.end() && img->second.fmt != x.fmt) o.str("fmt", nm(x.fmt));
    return o.done();
}

std::string sampler_obj(VkSampler s) {
    auto it = objs().samplers.find(h64(s));
    if (it == objs().samplers.end()) return "null";
    auto& c = it->second;
    Obj o;
    o.str("mag", nm(c.magFilter)).str("min", nm(c.minFilter)).str("mip", nm(c.mipmapMode));
    o.raw("addr", arr({q(nm(c.addressModeU)), q(nm(c.addressModeV)), q(nm(c.addressModeW))}));
    o.n("aniso", c.anisotropyEnable ? c.maxAnisotropy : 0).n("bias", c.mipLodBias).raw("lod", arr({num(c.minLod), num(c.maxLod)}));
    if (c.compareEnable) o.str("cmp", nm(c.compareOp));
    o.str("border", nm(c.borderColor));
    return o.done();
}

std::string buffer_obj(uint64_t buffer) {
    auto it = objs().buffers.find(buffer);
    if (it == objs().buffers.end()) return "null";
    Obj o;
    o.n("buffer", (double)it->second.size);
    std::string n = name_of(buffer);
    if (!n.empty()) o.str("name", n);
    return o.done();
}

static std::string mask(VkColorComponentFlags m) {
    std::string s;
    for (auto [bit, c] : {std::pair{VK_COLOR_COMPONENT_R_BIT, 'R'}, {VK_COLOR_COMPONENT_G_BIT, 'G'}, {VK_COLOR_COMPONENT_B_BIT, 'B'}, {VK_COLOR_COMPONENT_A_BIT, 'A'}})
        if (m & bit) s += c;
    return q(s);
}

static std::string eq(VkBlendFactor s, VkBlendFactor d, VkBlendOp op) { return q(nm(s) + "," + nm(d) + "," + nm(op)); }

// Pipeline state with the command buffer's dynamic values where the pipeline declared them dynamic.
std::string state_obj(const PipeInfo& p, const Dyn& y) {
    auto dyn = [&](VkDynamicState s, uint32_t bit) { return p.has_dyn(s) && (y.set & bit); };
    Obj o;
    o.str("topo", nm(dyn(VK_DYNAMIC_STATE_PRIMITIVE_TOPOLOGY, D_TOPO) ? y.topo : p.topo)).n("restart", p.restart);
    o.str("poly", nm(dyn(VK_DYNAMIC_STATE_POLYGON_MODE_EXT, D_POLY) ? y.poly : p.rs.polygonMode));
    o.str("cull", cull_name(dyn(VK_DYNAMIC_STATE_CULL_MODE, D_CULL) ? y.cull : p.rs.cullMode));
    o.str("front", nm(dyn(VK_DYNAMIC_STATE_FRONT_FACE, D_FRONT) ? y.front : p.rs.frontFace));
    o.n("dtest", dyn(VK_DYNAMIC_STATE_DEPTH_TEST_ENABLE, D_DTEST) ? y.dTest : p.ds.depthTestEnable);
    o.n("dwrite", dyn(VK_DYNAMIC_STATE_DEPTH_WRITE_ENABLE, D_DWRITE) ? y.dWrite : p.ds.depthWriteEnable);
    o.str("dcmp", nm(dyn(VK_DYNAMIC_STATE_DEPTH_COMPARE_OP, D_DCMP) ? y.dCmp : p.ds.depthCompareOp));
    bool biasEn = dyn(VK_DYNAMIC_STATE_DEPTH_BIAS_ENABLE, D_BIASEN) ? y.biasEn : p.rs.depthBiasEnable;
    if (biasEn) {
        bool d = dyn(VK_DYNAMIC_STATE_DEPTH_BIAS, D_BIAS);
        o.raw("bias", arr({num(d ? y.bias[0] : p.rs.depthBiasConstantFactor), num(d ? y.bias[1] : p.rs.depthBiasClamp),
                           num(d ? y.bias[2] : p.rs.depthBiasSlopeFactor)}));
    }
    o.n("stencil", dyn(VK_DYNAMIC_STATE_STENCIL_TEST_ENABLE, D_STEST) ? y.sTest : p.ds.stencilTestEnable);
    if (p.rs.depthClampEnable) o.n("clamp", 1);
    if (p.rs.rasterizerDiscardEnable) o.n("discard", 1);
    o.n("samples", p.samples);
    if (p.a2c) o.n("a2c", 1);
    std::vector<std::string> blend;
    size_t n = std::max({p.blend.size(), y.blendEn.size(), y.mask.size()});
    for (size_t i = 0; i < n; i++) {
        Blend b = i < p.blend.size() ? p.blend[i] : Blend{};
        if (dyn(VK_DYNAMIC_STATE_COLOR_BLEND_ENABLE_EXT, D_BLENDEN) && i < y.blendEn.size()) b.en = y.blendEn[i];
        if (dyn(VK_DYNAMIC_STATE_COLOR_WRITE_MASK_EXT, D_MASK) && i < y.mask.size()) b.mask = y.mask[i];
        if (dyn(VK_DYNAMIC_STATE_COLOR_BLEND_EQUATION_EXT, D_EQ) && i < y.eq.size()) {
            auto& e = y.eq[i];
            b.sc = e.srcColorBlendFactor; b.dc = e.dstColorBlendFactor; b.co = e.colorBlendOp;
            b.sa = e.srcAlphaBlendFactor; b.da = e.dstAlphaBlendFactor; b.ao = e.alphaBlendOp;
        }
        Obj x;
        x.n("en", b.en);
        if (b.en) x.raw("c", eq(b.sc, b.dc, b.co)).raw("a", eq(b.sa, b.da, b.ao));
        x.raw("mask", mask(b.mask));
        blend.push_back(x.done());
    }
    o.raw("blend", arr(blend));
    if (p.has_dyn(VK_DYNAMIC_STATE_VIEWPORT) || p.has_dyn(VK_DYNAMIC_STATE_VIEWPORT_WITH_COUNT))
        if (y.set & D_VIEWPORT) o.raw("vp", arr({num(y.vp.x), num(y.vp.y), num(y.vp.width), num(y.vp.height), num(y.vp.minDepth), num(y.vp.maxDepth)}));
    if (p.has_dyn(VK_DYNAMIC_STATE_SCISSOR) || p.has_dyn(VK_DYNAMIC_STATE_SCISSOR_WITH_COUNT))
        if (y.set & D_SCISSOR) o.raw("sc", arr({num(y.sc.offset.x), num(y.sc.offset.y), num(y.sc.extent.width), num(y.sc.extent.height)}));
    return o.done();
}

std::string pipe_obj(const PipeInfo& p, int id, uint64_t handle) {
    Obj o;
    o.str("t", "pipe").n("id", id).n("compute", p.compute);
    std::string n = name_of(handle);
    if (!n.empty()) o.str("name", n);
    std::vector<std::string> st;
    for (auto& s : p.stages) {
        Obj x;
        x.str("stage", nm(s.stage)).str("hash", hex(s.sh.hash)).n("size", (double)s.sh.size).str("entry", s.entry);
        if (!s.sh.src.empty()) x.str("src", s.sh.src);
        st.push_back(x.done());
    }
    o.raw("stages", arr(st));
    std::vector<std::string> vb, va, fm, dy, sets, pc;
    for (auto& b : p.vb) vb.push_back(arr({num(b.binding), num(b.stride), num(b.inputRate)}));
    for (auto& a : p.va) va.push_back(arr({num(a.location), num(a.binding), q(nm(a.format)), num(a.offset)}));
    for (auto f : p.colorFmts) fm.push_back(q(nm(f)));
    for (auto d : p.dyn) dy.push_back(q(nm(d)));
    o.raw("vb", arr(vb)).raw("va", arr(va)).raw("fmts", arr(fm)).raw("dyn", arr(dy));
    if (p.depthFmt) o.str("depth", nm(p.depthFmt));
    auto pl = objs().pipeLayouts.find(h64(p.layout));
    if (pl != objs().pipeLayouts.end()) {
        for (auto sl : pl->second.sets) {
            std::vector<std::string> bs;
            auto it = objs().setLayouts.find(h64(sl));
            if (it != objs().setLayouts.end())
                for (auto& b : it->second.bindings) bs.push_back(arr({num(b.binding), q(nm(b.type)), num(b.count)}));
            sets.push_back(arr(bs));
        }
        for (auto& r : pl->second.pc) pc.push_back(arr({num(r.stageFlags), num(r.offset), num(r.size)}));
    }
    o.raw("sets", arr(sets)).raw("pc", arr(pc));
    return o.done();
}

}  // namespace gpu
