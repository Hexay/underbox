// A submitted command buffer's events as JSON lines, resolved against device memory after the submit.
#include "json.h"
#include <algorithm>
#include <cstdlib>
#include <cstring>

namespace gpu {

static constexpr uint32_t kBindless = 16;          // larger arrays are listed once per frame, not per draw
static constexpr size_t kBlobMax = 64 * 1024;      // uniform contents written out up to this size
static constexpr size_t kHashMax = 4 * 1024 * 1024; // storage buffers hashed up to this size
static constexpr size_t kStoreBlobMax = 512 * 1024; // and written out up to this size

static void line(Frame& fr, const std::string& s) { fputs(s.c_str(), fr.f); fputc('\n', fr.f); }

static int pipe_id(Frame& fr, VkPipeline p) {
    if (!p) return -1;
    auto it = fr.pipeIds.find(h64(p));
    if (it != fr.pipeIds.end()) return it->second;
    int id = (int)fr.pipeIds.size();
    fr.pipeIds[h64(p)] = id;
    auto pi = pipes().find(h64(p));
    if (pi != pipes().end()) line(fr, pipe_obj(*pi->second, id, h64(p)));
    return id;
}

static std::string put_blob(Frame& fr, const std::vector<uint8_t>& bytes, size_t max) {
    uint64_t h = fnv(bytes.data(), bytes.size());
    if (bytes.size() <= max && fr.blobs.insert(h).second)
        line(fr, "{\"t\":\"blob\",\"h\":\"" + hex(h) + "\",\"hex\":\"" + hexbytes(bytes.data(), bytes.size()) + "\"}");
    return q(hex(h));
}

static std::string blob(Device* d, Frame& fr, VkBuffer b, VkDeviceSize off, VkDeviceSize range) {
    auto bytes = read_buffer(d, b, off, range);
    return bytes.empty() ? "null" : put_blob(fr, bytes, kBlobMax);
}

static std::string bindless(Frame& fr, const void* key, const std::vector<Desc>& v, const LayoutBinding& lb) {
    std::string k = hex((uint64_t)key ^ lb.binding);
    if (fr.bindless.insert({(uint64_t)key, lb.binding}).second) {
        // Filled slots by index: shaders select them by the index, so it is the identity that matters.
        std::vector<std::string> slots;
        size_t filled = 0;
        for (size_t i = 0; i < v.size(); i++) {
            auto& x = v[i];
            if (!x.view && !x.buf && !x.sampler) continue;
            filled++;
            if (x.view) slots.push_back(arr({num((double)i), view_obj(x.view)}));
            else if (x.sampler) slots.push_back(arr({num((double)i), sampler_obj(x.sampler)}));
        }
        line(fr, "{\"t\":\"bindless\",\"key\":\"" + k + "\",\"type\":" + q(nm(lb.type)) + ",\"count\":" + num(lb.count) +
                     ",\"filled\":" + num((double)filled) + ",\"slots\":" + arr(slots) + "}");
    }
    return q(k);
}

static std::string resources(Device* d, Frame& fr, const PipeInfo& p, const BindPoint& bp) {
    std::vector<std::string> out;
    auto pl = objs().pipeLayouts.find(h64(p.layout));
    if (pl == objs().pipeLayouts.end()) return "[]";
    for (uint32_t s = 0; s < pl->second.sets.size(); s++) {
        auto sl = objs().setLayouts.find(h64(pl->second.sets[s]));
        if (sl == objs().setLayouts.end() || s >= bp.sets.size()) continue;
        const Bound& b = bp.sets[s];
        std::shared_ptr<SetData> sd = b.pushed;
        if (!sd) { auto it = sets().find(h64(b.set)); if (it != sets().end()) sd = it->second; }
        if (!sd) continue;
        size_t dynUsed = 0;
        for (auto& lb : sl->second.bindings) {
            auto bi = sd->b.find(lb.binding);
            if (bi == sd->b.end()) continue;
            bool dynamic = lb.type == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER_DYNAMIC || lb.type == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER_DYNAMIC;
            Obj o;
            o.n("s", s).n("b", lb.binding).str("type", nm(lb.type));
            if (lb.count > kBindless) {
                o.raw("bindless", bindless(fr, sd.get(), bi->second, lb));
                out.push_back(o.done());
                continue;
            }
            std::vector<std::string> el;
            for (auto& x : bi->second) {
                Obj e;
                VkDeviceSize off = x.off + (dynamic && dynUsed < b.offs.size() ? b.offs[dynUsed++] : 0);
                switch (lb.type) {
                case VK_DESCRIPTOR_TYPE_SAMPLER: e.raw("smp", sampler_obj(x.sampler)); break;
                case VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER: e.raw("view", view_obj(x.view)).raw("smp", sampler_obj(x.sampler)); break;
                case VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE: case VK_DESCRIPTOR_TYPE_STORAGE_IMAGE: case VK_DESCRIPTOR_TYPE_INPUT_ATTACHMENT:
                    e.raw("view", view_obj(x.view)).str("layout", nm(x.layout)); break;
                case VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER: case VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER_DYNAMIC: {
                    auto bytes = x.buf ? read_buffer(d, x.buf, off, x.range).size() : 0;
                    e.n("size", (double)bytes).raw("blob", x.buf ? blob(d, fr, x.buf, off, x.range) : "null");
                    break;
                }
                case VK_DESCRIPTOR_TYPE_STORAGE_BUFFER: case VK_DESCRIPTOR_TYPE_STORAGE_BUFFER_DYNAMIC: {
                    auto info = objs().buffers.find(h64(x.buf));
                    VkDeviceSize n = x.range != VK_WHOLE_SIZE ? x.range : info != objs().buffers.end() && off < info->second.size ? info->second.size - off : 0;
                    e.n("size", (double)n);
                    if (n && n <= kHashMax) {
                        auto bytes = read_buffer(d, x.buf, off, n);
                        // Small storage buffers keep their contents: per-draw data (bindless indices) can live there.
                        if (!bytes.empty()) e.raw("sblob", put_blob(fr, bytes, kStoreBlobMax));
                    }
                    break;
                }
                default: break;
                }
                el.push_back(e.done());
            }
            o.raw("el", arr(el));
            out.push_back(o.done());
        }
    }
    return arr(out);
}

struct Sub { uint32_t n, inst, first; int32_t vofs; uint32_t finst; };

static std::vector<Sub> subs(Device* d, const DrawRec& r) {
    std::vector<Sub> v;
    if (r.indirect) {
        uint32_t stride = r.istride ? r.istride : (r.indexed ? 20 : 16);
        for (uint32_t i = 0; i < r.icount && i < 256; i++) {
            auto a = read_buffer(d, r.indirect, r.ioff + (VkDeviceSize)i * stride, r.indexed ? 20 : 16);
            if (a.empty()) break;
            uint32_t w[5]{};
            memcpy(w, a.data(), a.size());
            if (r.indexed) v.push_back({w[0], w[1], w[2], (int32_t)w[3], w[4]});
            else v.push_back({w[0], w[1], w[2], 0, w[3]});
        }
    } else if (!r.multi.empty()) {
        for (auto& m : r.multi) v.push_back({m.indexCount, r.inst, m.firstIndex, m.vertexOffset, r.finst});
    } else {
        v.push_back({r.n, r.inst, r.first, r.vofs, r.finst});
    }
    return v;
}

// Index range and a hash of the indices relative to their minimum: a mesh's identity across engines.
static void index_info(Device* d, const State& st, const Sub& s, Obj& o) {
    uint32_t isz = st.ibType == VK_INDEX_TYPE_UINT32 ? 4 : st.ibType == VK_INDEX_TYPE_UINT8 ? 1 : 2;
    auto bytes = read_buffer(d, st.ib, st.ibOff + (VkDeviceSize)s.first * isz, (VkDeviceSize)s.n * isz);
    if (bytes.empty() || !s.n) return;
    std::vector<uint32_t> idx(s.n);
    uint32_t lo = UINT32_MAX, hi = 0, restart = isz == 4 ? UINT32_MAX : isz == 2 ? 0xffff : 0xff;
    for (uint32_t i = 0; i < s.n; i++) {
        uint32_t x = 0;
        memcpy(&x, bytes.data() + (size_t)i * isz, isz);
        idx[i] = x;
        if (x != restart) { lo = std::min(lo, x); hi = std::max(hi, x); }
    }
    for (auto& x : idx) x -= lo;
    o.n("vmin", lo).n("vspan", hi - lo + 1).str("ih", hex(fnv(idx.data(), idx.size() * 4)));
}

static void draw(Device* d, Frame& fr, const DrawRec& r, int index) {
    const State& st = r.st;
    Obj o;
    o.str("t", "draw").n("p", fr.pass).n("i", index).str("cmd", r.cmd);
    if (!r.label.empty()) o.str("label", r.label);
    if (r.skipped) o.n("skipped", 1);
    o.n("pipe", pipe_id(fr, st.gfx.pipe));
    std::vector<std::string> sv;
    auto all = subs(d, r);
    for (auto& s : all) {
        Obj x;
        x.n("n", s.n).n("inst", s.inst).n("first", s.first);
        if (s.vofs) x.n("vofs", s.vofs);
        if (s.finst) x.n("finst", s.finst);
        if (r.indexed) index_info(d, st, s, x);
        sv.push_back(x.done());
    }
    o.raw("sub", arr(sv));
    if (r.indexed) o.str("ib", nm(st.ibType));
    if (!st.pc.empty()) o.str("pc", hexbytes(st.pc.data(), st.pc.size()));
    auto pi = pipes().find(h64(st.gfx.pipe));
    if (pi != pipes().end()) {
        std::vector<std::string> vb, ivb, vvb;
        static const VkDeviceSize verts = [] { const char* e = getenv("UNDERBOX_GPU_VERTS"); return e ? (VkDeviceSize)strtoull(e, nullptr, 10) : 0; }();
        for (auto& b : pi->second->vb) {
            if (b.binding >= st.vbs.size()) continue;
            auto& v = st.vbs[b.binding];
            auto info = objs().buffers.find(h64(v.buf));
            VkDeviceSize size = info != objs().buffers.end() && v.off < info->second.size ? info->second.size - v.off : 0;
            VkDeviceSize stride = v.stride ? v.stride : b.stride;
            vb.push_back(arr({num(b.binding), num((double)stride), num((double)size)}));
            if (b.inputRate == VK_VERTEX_INPUT_RATE_VERTEX && verts && !all.empty() && stride) {
                // The first UNDERBOX_GPU_VERTS vertices from the first draw's vertex offset: [binding, stride, first, blob].
                VkDeviceSize first = (VkDeviceSize)std::max(0, all[0].vofs);
                VkDeviceSize left = size > first * stride ? size - first * stride : 0;
                auto bytes = left ? read_buffer(d, v.buf, v.off + first * stride, std::min<VkDeviceSize>(left, verts * stride)) : std::vector<uint8_t>();
                if (!bytes.empty()) vvb.push_back(arr({num(b.binding), num((double)stride), num((double)first), put_blob(fr, bytes, kBlobMax)}));
            }
            if (b.inputRate != VK_VERTEX_INPUT_RATE_INSTANCE || all.empty() || !stride) continue;
            // Per-instance data of the instances drawn: [binding, stride, first instance, blob].
            uint32_t lo = UINT32_MAX, hi = 0;
            for (auto& s : all) { lo = std::min(lo, s.finst); hi = std::max(hi, s.finst + s.inst); }
            if (hi <= lo) continue;
            auto bytes = read_buffer(d, v.buf, v.off + lo * stride, (VkDeviceSize)(hi - lo) * stride);
            if (!bytes.empty()) ivb.push_back(arr({num(b.binding), num((double)stride), num(lo), put_blob(fr, bytes, kBlobMax)}));
        }
        o.raw("vb", arr(vb));
        if (!ivb.empty()) o.raw("ivb", arr(ivb));
        if (!vvb.empty()) o.raw("vvb", arr(vvb));
        o.raw("state", state_obj(*pi->second, st.dyn)).raw("res", resources(d, fr, *pi->second, st.gfx));
    }
    line(fr, o.done());
    fr.draws++;
}

static void dispatch(Device* d, Frame& fr, const DrawRec& r) {
    Obj o;
    o.str("t", "dispatch").n("p", fr.pass).str("cmd", r.cmd);
    if (!r.label.empty()) o.str("label", r.label);
    o.n("pipe", pipe_id(fr, r.st.comp.pipe));
    uint32_t g[3] = {r.group[0], r.group[1], r.group[2]};
    if (r.indirect) { auto a = read_buffer(d, r.indirect, r.ioff, 12); if (a.size() == 12) memcpy(g, a.data(), 12); }
    o.raw("group", arr({num(g[0]), num(g[1]), num(g[2])}));
    if (!r.st.pc.empty()) o.str("pc", hexbytes(r.st.pc.data(), r.st.pc.size()));
    auto pi = pipes().find(h64(r.st.comp.pipe));
    if (pi != pipes().end()) o.raw("res", resources(d, fr, *pi->second, r.st.comp));
    line(fr, o.done());
}

static std::string clear_value(const Att& a) {
    if (a.depth) return arr({num(a.clear.depthStencil.depth), num(a.clear.depthStencil.stencil)});
    auto& c = a.clear.color.float32;
    return arr({num(c[0]), num(c[1]), num(c[2]), num(c[3])});
}

static void pass(Frame& fr, PassRec& p) {
    fr.pass++;
    dump_submit_order(p, true);
    Obj o;
    o.str("t", "pass").n("i", fr.pass).str("sel", p.sel).n("dynamic", p.dynamic);
    if (!p.label.empty()) o.str("label", p.label);
    if (p.chosen) o.n("dumped", 1);
    o.raw("area", arr({num(p.area.offset.x), num(p.area.offset.y), num(p.area.extent.width), num(p.area.extent.height)}));
    std::vector<std::string> atts;
    for (auto& a : p.atts) {
        Obj x;
        x.raw("view", view_obj(a.view)).str("fmt", nm(a.fmt)).n("depth", a.depth).str("load", nm(a.load)).str("store", nm(a.store)).str("layout", nm(a.layout));
        if (a.load == VK_ATTACHMENT_LOAD_OP_CLEAR) x.raw("clear", clear_value(a));
        atts.push_back(x.done());
    }
    o.raw("att", arr(atts));
    line(fr, o.done());
}

void emit_cmdbuf(Device* d, Frame& fr, VkCommandBuffer cb) {
    CmdRec* r = rec(cb);
    if (!r) { fr.unrecorded++; line(fr, "{\"t\":\"unrecorded\"}"); return; }
    int index = 0;
    for (auto& e : r->ev) {
        switch (e.k) {
        case Event::Pass: pass(fr, *e.pass); index = 0; break;
        case Event::End: line(fr, "{\"t\":\"end\",\"p\":" + num(fr.pass) + "}"); break;
        case Event::Draw: draw(d, fr, *e.draw, index++); break;
        case Event::Dispatch: dispatch(d, fr, *e.draw); break;
        case Event::Xfer: {
            bool image = objs().images.count(e.dst);
            line(fr, "{\"t\":\"xfer\",\"op\":" + q(e.text) + ",\"dst\":" + (image ? image_obj(e.dst) : buffer_obj(e.dst)) + "}");
            break;
        }
        case Event::Clear: line(fr, "{\"t\":\"clear\",\"p\":" + num(fr.pass) + ",\"text\":" + q(e.text) + "}"); break;
        case Event::Label: line(fr, "{\"t\":\"label\",\"text\":" + q(e.text) + "}"); break;
        case Event::Exec:
            for (auto s : e.exec) emit_cmdbuf(d, fr, s);
            break;
        }
    }
}

}  // namespace gpu
