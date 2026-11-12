// Render-target dumps: UNDERBOX_GPU_DUMP=<pass index>[:<draw>] copies out the attachments of the pass at
// that index in submit order (the listing's P<n>) at its end; with a draw index, the pass's later draws
// are dropped so the targets hold the state after that draw.
// Record order isn't stable on multi-threaded engines, submit order is. So a lead frame learns the
// target's attachment signature (its sel) and position among same-sel passes in submit order; later
// frames inject copies into every pass with that sel (and drop their late draws: a caveat when an
// earlier same-sel pass feeds the target), and the capture keeps the copy at the learned position.
#include "capture.h"
#include <cstdlib>
#include <cstring>

using namespace gpu;

static int g_target = -1, g_draw = -1;
static std::string g_sig;
static int g_sigK = -1;
// Submit-order counters for the frame being learned or captured.
static int g_index = 0;
static std::unordered_map<std::string, int> g_seen;

static void parse_env() {
    static bool done = false;
    if (done) return;
    done = true;
    const char* e = getenv("UNDERBOX_GPU_DUMP");
    if (!e || !*e) return;
    g_target = atoi(e + (*e == 'P'));
    if (const char* c = strchr(e, ':')) g_draw = atoi(c + 1);
}

namespace gpu {

void dump_new_frame() { g_index = 0; g_seen.clear(); }

void dump_submit_order(PassRec& p, bool capturing) {
    int k = g_seen[p.sel]++;
    if (!capturing && g_index == g_target) { g_sig = p.sel; g_sigK = k; }
    if (capturing) p.chosen = p.dumped && p.sel == g_sig && k == g_sigK;
    g_index++;
}

static VkExtent2D extent_of(VkImageView v, const ImageInfo** img, const ViewInfo** view) {
    auto vi = objs().views.find(h64(v));
    if (vi == objs().views.end()) return {0, 0};
    auto ii = objs().images.find(h64(vi->second.image));
    if (ii == objs().images.end()) return {0, 0};
    *img = &ii->second;
    *view = &vi->second;
    uint32_t m = vi->second.range.baseMipLevel;
    return {std::max(1u, ii->second.ext.width >> m), std::max(1u, ii->second.ext.height >> m)};
}

void dump_begin_pass(PassRec& p) {
    parse_env();
    uint64_t h = fnv("", 0);
    for (auto& a : p.atts) {
        const ImageInfo* img = nullptr;
        const ViewInfo* view = nullptr;
        VkExtent2D e = extent_of(a.view, &img, &view);
        uint32_t key[5] = {(uint32_t)a.fmt, (uint32_t)a.samples, (uint32_t)a.load, e.width, e.height};
        h = fnv(key, sizeof key, h);
    }
    p.sel = hex(h).substr(0, 8);
    if (g_sigK >= 0 && p.sel == g_sig) { p.dumped = true; p.dumpDraw = g_draw; }
}

static uint32_t host_type(Device* d, uint32_t bits) {
    for (uint32_t i = 0; i < d->mem.memoryTypeCount; i++) {
        auto f = d->mem.memoryTypes[i].propertyFlags;
        if ((bits & (1u << i)) && (f & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT) && (f & VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)) return i;
    }
    return UINT32_MAX;
}

static void barrier(Device* d, VkCommandBuffer cb, VkImage img, VkImageAspectFlags aspect, const VkImageSubresourceRange& r,
                    VkImageLayout from, VkImageLayout to) {
    VkImageMemoryBarrier b{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    b.srcAccessMask = VK_ACCESS_MEMORY_WRITE_BIT;
    b.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    b.oldLayout = from;
    b.newLayout = to;
    b.srcQueueFamilyIndex = b.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    b.image = img;
    b.subresourceRange = {aspect, r.baseMipLevel, 1, r.baseArrayLayer, 1};
    d->CmdPipelineBarrier(cb, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &b);
}

void dump_end_pass(Device* d, VkCommandBuffer cb, CmdRec& r) {
    if (!r.pass->dumped) return;
    for (uint32_t i = 0; i < r.pass->atts.size(); i++) {
        Att& a = r.pass->atts[i];
        const ImageInfo* img = nullptr;
        const ViewInfo* view = nullptr;
        VkExtent2D e = extent_of(a.view, &img, &view);
        uint32_t tb = texel_bytes(a.fmt);
        if (!img || !tb || img->samples != VK_SAMPLE_COUNT_1_BIT || a.layout == VK_IMAGE_LAYOUT_UNDEFINED) continue;
        VkBufferCreateInfo bi{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
        bi.size = (VkDeviceSize)e.width * e.height * tb;
        bi.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
        DumpReq q{r.pass, i, (uint32_t)r.pass->dumpDraw, VK_NULL_HANDLE, VK_NULL_HANDLE, a.fmt, e.width, e.height};
        if (d->CreateBuffer(d->dev, &bi, nullptr, &q.buf) != VK_SUCCESS) continue;
        VkMemoryRequirements mr;
        d->GetBufferMemoryRequirements(d->dev, q.buf, &mr);
        VkMemoryAllocateInfo ai{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, nullptr, mr.size, host_type(d, mr.memoryTypeBits)};
        if (ai.memoryTypeIndex == UINT32_MAX || d->AllocateMemory(d->dev, &ai, nullptr, &q.mem) != VK_SUCCESS) {
            d->DestroyBuffer(d->dev, q.buf, nullptr);
            continue;
        }
        d->BindBufferMemory(d->dev, q.buf, q.mem, 0);
        VkImageAspectFlags all = a.depth ? VK_IMAGE_ASPECT_DEPTH_BIT : VK_IMAGE_ASPECT_COLOR_BIT;
        if (a.fmt == VK_FORMAT_D24_UNORM_S8_UINT || a.fmt == VK_FORMAT_D32_SFLOAT_S8_UINT) all |= VK_IMAGE_ASPECT_STENCIL_BIT;
        VkImageAspectFlags copy = a.depth ? VK_IMAGE_ASPECT_DEPTH_BIT : VK_IMAGE_ASPECT_COLOR_BIT;
        VkImage image = view->image;
        barrier(d, cb, image, all, view->range, a.layout, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
        VkBufferImageCopy c{};
        c.imageSubresource = {copy, view->range.baseMipLevel, view->range.baseArrayLayer, 1};
        c.imageExtent = {e.width, e.height, 1};
        d->CmdCopyImageToBuffer(cb, image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, q.buf, 1, &c);
        barrier(d, cb, image, all, view->range, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, a.layout);
        r.dumps.push_back(q);
    }
}

void dump_write(Device* d, CmdRec& r, FILE* out, const std::string& dir) {
    for (auto& q : r.dumps) {
        void* p = nullptr;
        std::string file = "dump-P" + std::to_string(g_target) + "-" + std::to_string(q.att) + ".raw";
        if (out && q.pass->chosen && d->MapMemory(d->dev, q.mem, 0, VK_WHOLE_SIZE, 0, &p) == VK_SUCCESS) {
            if (FILE* f = fopen((dir + "/" + file).c_str(), "wb")) {
                fwrite(p, 1, (size_t)q.w * q.h * texel_bytes(q.fmt), f);
                fclose(f);
            }
            d->UnmapMemory(d->dev, q.mem);
            fprintf(out, "{\"t\":\"dump\",\"pass\":%d,\"sel\":\"%s\",\"att\":%u,\"draw\":%d,\"fmt\":\"%s\",\"w\":%u,\"h\":%u,\"file\":\"%s\"}\n",
                    g_target, q.pass->sel.c_str(), q.att, (int)q.draw, nm(q.fmt).c_str(), q.w, q.h, file.c_str());
        }
        d->DestroyBuffer(d->dev, q.buf, nullptr);
        d->FreeMemory(d->dev, q.mem, nullptr);
    }
    r.dumps.clear();
}

}  // namespace gpu
