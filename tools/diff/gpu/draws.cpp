// Draw, dispatch, dynamic-state, clear and transfer commands.
#include "capture.h"
#include <cstring>

using namespace gpu;

static std::shared_ptr<DrawRec> add(CmdRec& r, const char* cmd, bool dispatch) {
    auto d = std::make_shared<DrawRec>();
    d->cmd = cmd;
    d->st = r.st;
    d->label = label_of(r);
    if (!dispatch) {
        // A render-target dump of draw k drops the pass's later draws (dump.cpp).
        if (r.pass && r.pass->dumpDraw >= 0 && (int)r.drawInPass > r.pass->dumpDraw) d->skipped = true;
        r.drawInPass++;
    }
    r.ev.push_back({dispatch ? Event::Dispatch : Event::Draw, "", nullptr, d});
    return d;
}

#define UB_DRAW(cmd, body, call) \
    bool skip = false; \
    with(cb, [&](CmdRec& r) { auto d = add(r, cmd, false); body; skip = d->skipped; }); \
    if (!skip) device(cb)->call;

void hk_CmdDraw(VkCommandBuffer cb, uint32_t n, uint32_t inst, uint32_t first, uint32_t finst) {
    UB_DRAW("draw", (d->n = n, d->inst = inst, d->first = first, d->finst = finst), CmdDraw(cb, n, inst, first, finst))
}

void hk_CmdDrawIndexed(VkCommandBuffer cb, uint32_t n, uint32_t inst, uint32_t first, int32_t vofs, uint32_t finst) {
    UB_DRAW("drawIndexed", (d->n = n, d->inst = inst, d->first = first, d->vofs = vofs, d->finst = finst, d->indexed = true),
            CmdDrawIndexed(cb, n, inst, first, vofs, finst))
}

void hk_CmdDrawIndirect(VkCommandBuffer cb, VkBuffer b, VkDeviceSize o, uint32_t n, uint32_t stride) {
    UB_DRAW("drawIndirect", (d->indirect = b, d->ioff = o, d->icount = n, d->istride = stride), CmdDrawIndirect(cb, b, o, n, stride))
}

void hk_CmdDrawIndexedIndirect(VkCommandBuffer cb, VkBuffer b, VkDeviceSize o, uint32_t n, uint32_t stride) {
    UB_DRAW("drawIndexedIndirect", (d->indirect = b, d->ioff = o, d->icount = n, d->istride = stride, d->indexed = true),
            CmdDrawIndexedIndirect(cb, b, o, n, stride))
}

void hk_CmdDrawIndirectCount(VkCommandBuffer cb, VkBuffer b, VkDeviceSize o, VkBuffer cbuf, VkDeviceSize co, uint32_t n, uint32_t stride) {
    UB_DRAW("drawIndirectCount", (d->indirect = b, d->ioff = o, d->icount = n, d->istride = stride),
            CmdDrawIndirectCount(cb, b, o, cbuf, co, n, stride))
}

void hk_CmdDrawIndexedIndirectCount(VkCommandBuffer cb, VkBuffer b, VkDeviceSize o, VkBuffer cbuf, VkDeviceSize co, uint32_t n, uint32_t stride) {
    UB_DRAW("drawIndexedIndirectCount", (d->indirect = b, d->ioff = o, d->icount = n, d->istride = stride, d->indexed = true),
            CmdDrawIndexedIndirectCount(cb, b, o, cbuf, co, n, stride))
}

void hk_CmdDrawMultiEXT(VkCommandBuffer cb, uint32_t n, const VkMultiDrawInfoEXT* info, uint32_t inst, uint32_t finst, uint32_t stride) {
    UB_DRAW("drawMulti", ({
                d->inst = inst; d->finst = finst;
                for (uint32_t i = 0; i < n; i++) {
                    auto m = (const VkMultiDrawInfoEXT*)((const uint8_t*)info + i * stride);
                    d->multi.push_back({m->firstVertex, m->vertexCount, 0});
                }
            }),
            CmdDrawMultiEXT(cb, n, info, inst, finst, stride))
}

void hk_CmdDrawMultiIndexedEXT(VkCommandBuffer cb, uint32_t n, const VkMultiDrawIndexedInfoEXT* info, uint32_t inst, uint32_t finst,
                               uint32_t stride, const int32_t* vofs) {
    UB_DRAW("drawMultiIndexed", ({
                d->inst = inst; d->finst = finst; d->indexed = true;
                for (uint32_t i = 0; i < n; i++) {
                    auto m = *(const VkMultiDrawIndexedInfoEXT*)((const uint8_t*)info + i * stride);
                    if (vofs) m.vertexOffset = *vofs;
                    d->multi.push_back(m);
                }
            }),
            CmdDrawMultiIndexedEXT(cb, n, info, inst, finst, stride, vofs))
}

void hk_CmdDispatch(VkCommandBuffer cb, uint32_t x, uint32_t y, uint32_t z) {
    with(cb, [&](CmdRec& r) { auto d = add(r, "dispatch", true); d->group[0] = x; d->group[1] = y; d->group[2] = z; });
    device(cb)->CmdDispatch(cb, x, y, z);
}

void hk_CmdDispatchIndirect(VkCommandBuffer cb, VkBuffer b, VkDeviceSize o) {
    with(cb, [&](CmdRec& r) { auto d = add(r, "dispatchIndirect", true); d->indirect = b; d->ioff = o; });
    device(cb)->CmdDispatchIndirect(cb, b, o);
}

#define UB_SET(bit, assign) with(cb, [&](CmdRec& r) { Dyn& y = r.st.dyn; y.set |= bit; assign; });

void hk_CmdSetViewport(VkCommandBuffer cb, uint32_t first, uint32_t n, const VkViewport* v) {
    if (first == 0 && n) UB_SET(D_VIEWPORT, y.vp = v[0])
    device(cb)->CmdSetViewport(cb, first, n, v);
}
void hk_CmdSetScissor(VkCommandBuffer cb, uint32_t first, uint32_t n, const VkRect2D* s) {
    if (first == 0 && n) UB_SET(D_SCISSOR, y.sc = s[0])
    device(cb)->CmdSetScissor(cb, first, n, s);
}
void hk_CmdSetCullMode(VkCommandBuffer cb, VkCullModeFlags m) { UB_SET(D_CULL, y.cull = m) device(cb)->CmdSetCullMode(cb, m); }
void hk_CmdSetFrontFace(VkCommandBuffer cb, VkFrontFace f) { UB_SET(D_FRONT, y.front = f) device(cb)->CmdSetFrontFace(cb, f); }
void hk_CmdSetPrimitiveTopology(VkCommandBuffer cb, VkPrimitiveTopology t) { UB_SET(D_TOPO, y.topo = t) device(cb)->CmdSetPrimitiveTopology(cb, t); }
void hk_CmdSetDepthTestEnable(VkCommandBuffer cb, VkBool32 e) { UB_SET(D_DTEST, y.dTest = e) device(cb)->CmdSetDepthTestEnable(cb, e); }
void hk_CmdSetDepthWriteEnable(VkCommandBuffer cb, VkBool32 e) { UB_SET(D_DWRITE, y.dWrite = e) device(cb)->CmdSetDepthWriteEnable(cb, e); }
void hk_CmdSetDepthCompareOp(VkCommandBuffer cb, VkCompareOp o) { UB_SET(D_DCMP, y.dCmp = o) device(cb)->CmdSetDepthCompareOp(cb, o); }
void hk_CmdSetDepthBiasEnable(VkCommandBuffer cb, VkBool32 e) { UB_SET(D_BIASEN, y.biasEn = e) device(cb)->CmdSetDepthBiasEnable(cb, e); }
void hk_CmdSetDepthBias(VkCommandBuffer cb, float c, float cl, float s) {
    UB_SET(D_BIAS, (y.bias[0] = c, y.bias[1] = cl, y.bias[2] = s))
    device(cb)->CmdSetDepthBias(cb, c, cl, s);
}
void hk_CmdSetStencilTestEnable(VkCommandBuffer cb, VkBool32 e) { UB_SET(D_STEST, y.sTest = e) device(cb)->CmdSetStencilTestEnable(cb, e); }
void hk_CmdSetStencilReference(VkCommandBuffer cb, VkStencilFaceFlags f, uint32_t v) { UB_SET(D_SREF, y.sref = v) device(cb)->CmdSetStencilReference(cb, f, v); }
void hk_CmdSetBlendConstants(VkCommandBuffer cb, const float c[4]) { UB_SET(D_BC, memcpy(y.bc, c, sizeof y.bc)) device(cb)->CmdSetBlendConstants(cb, c); }
void hk_CmdSetPolygonModeEXT(VkCommandBuffer cb, VkPolygonMode m) { UB_SET(D_POLY, y.poly = m) device(cb)->CmdSetPolygonModeEXT(cb, m); }

template <class T> static void put(std::vector<T>& v, uint32_t first, uint32_t n, const T* src) {
    if (v.size() < first + n) v.resize(first + n);
    for (uint32_t i = 0; i < n; i++) v[first + i] = src[i];
}
void hk_CmdSetColorBlendEnableEXT(VkCommandBuffer cb, uint32_t first, uint32_t n, const VkBool32* e) {
    UB_SET(D_BLENDEN, put(y.blendEn, first, n, e))
    device(cb)->CmdSetColorBlendEnableEXT(cb, first, n, e);
}
void hk_CmdSetColorWriteMaskEXT(VkCommandBuffer cb, uint32_t first, uint32_t n, const VkColorComponentFlags* m) {
    UB_SET(D_MASK, put(y.mask, first, n, m))
    device(cb)->CmdSetColorWriteMaskEXT(cb, first, n, m);
}
void hk_CmdSetColorBlendEquationEXT(VkCommandBuffer cb, uint32_t first, uint32_t n, const VkColorBlendEquationEXT* e) {
    UB_SET(D_EQ, put(y.eq, first, n, e))
    device(cb)->CmdSetColorBlendEquationEXT(cb, first, n, e);
}

void gpu::xfer(VkCommandBuffer cb, const char* op, uint64_t dst) {
    with(cb, [&](CmdRec& r) { r.ev.push_back({Event::Xfer, op, nullptr, nullptr, {}, dst}); });
}

void hk_CmdClearAttachments(VkCommandBuffer cb, uint32_t n, const VkClearAttachment* a, uint32_t nr, const VkClearRect* rects) {
    with(cb, [&](CmdRec& r) { r.ev.push_back({Event::Clear, std::to_string(n) + " attachments"}); });
    device(cb)->CmdClearAttachments(cb, n, a, nr, rects);
}
void hk_CmdClearColorImage(VkCommandBuffer cb, VkImage i, VkImageLayout l, const VkClearColorValue* c, uint32_t n, const VkImageSubresourceRange* rg) {
    xfer(cb, "clearColorImage", h64(i));
    device(cb)->CmdClearColorImage(cb, i, l, c, n, rg);
}
void hk_CmdClearDepthStencilImage(VkCommandBuffer cb, VkImage i, VkImageLayout l, const VkClearDepthStencilValue* c, uint32_t n,
                                  const VkImageSubresourceRange* rg) {
    xfer(cb, "clearDepthStencilImage", h64(i));
    device(cb)->CmdClearDepthStencilImage(cb, i, l, c, n, rg);
}
void hk_CmdCopyBuffer(VkCommandBuffer cb, VkBuffer s, VkBuffer d, uint32_t n, const VkBufferCopy* rg) {
    xfer(cb, "copyBuffer", h64(d));
    device(cb)->CmdCopyBuffer(cb, s, d, n, rg);
}
void hk_CmdCopyImage(VkCommandBuffer cb, VkImage s, VkImageLayout sl, VkImage d, VkImageLayout dl, uint32_t n, const VkImageCopy* rg) {
    xfer(cb, "copyImage", h64(d));
    device(cb)->CmdCopyImage(cb, s, sl, d, dl, n, rg);
}
void hk_CmdCopyImageToBuffer(VkCommandBuffer cb, VkImage s, VkImageLayout l, VkBuffer d, uint32_t n, const VkBufferImageCopy* rg) {
    xfer(cb, "copyImageToBuffer", h64(s));
    device(cb)->CmdCopyImageToBuffer(cb, s, l, d, n, rg);
}
void hk_CmdBlitImage(VkCommandBuffer cb, VkImage s, VkImageLayout sl, VkImage d, VkImageLayout dl, uint32_t n, const VkImageBlit* rg, VkFilter f) {
    xfer(cb, "blitImage", h64(d));
    device(cb)->CmdBlitImage(cb, s, sl, d, dl, n, rg, f);
}
void hk_CmdResolveImage(VkCommandBuffer cb, VkImage s, VkImageLayout sl, VkImage d, VkImageLayout dl, uint32_t n, const VkImageResolve* rg) {
    xfer(cb, "resolveImage", h64(d));
    device(cb)->CmdResolveImage(cb, s, sl, d, dl, n, rg);
}
void hk_CmdUpdateBuffer(VkCommandBuffer cb, VkBuffer d, VkDeviceSize o, VkDeviceSize n, const void* data) {
    xfer(cb, "updateBuffer", h64(d));
    device(cb)->CmdUpdateBuffer(cb, d, o, n, data);
}
void hk_CmdFillBuffer(VkCommandBuffer cb, VkBuffer d, VkDeviceSize o, VkDeviceSize n, uint32_t v) {
    xfer(cb, "fillBuffer", h64(d));
    device(cb)->CmdFillBuffer(cb, d, o, n, v);
}
void hk_CmdCopyBuffer2(VkCommandBuffer cb, const VkCopyBufferInfo2* i) { xfer(cb, "copyBuffer", h64(i->dstBuffer)); device(cb)->CmdCopyBuffer2(cb, i); }
void hk_CmdCopyImage2(VkCommandBuffer cb, const VkCopyImageInfo2* i) { xfer(cb, "copyImage", h64(i->dstImage)); device(cb)->CmdCopyImage2(cb, i); }
void hk_CmdCopyImageToBuffer2(VkCommandBuffer cb, const VkCopyImageToBufferInfo2* i) {
    xfer(cb, "copyImageToBuffer", h64(i->srcImage));
    device(cb)->CmdCopyImageToBuffer2(cb, i);
}
void hk_CmdBlitImage2(VkCommandBuffer cb, const VkBlitImageInfo2* i) { xfer(cb, "blitImage", h64(i->dstImage)); device(cb)->CmdBlitImage2(cb, i); }
