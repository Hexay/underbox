// Texture contents: buffer-to-image copies are tracked from load time on (not only in the captured frame)
// and hashed from their source buffer at submit, so each image can name what was uploaded into it.
#include "capture.h"

using namespace gpu;
using Lock = std::lock_guard<std::mutex>;

namespace {

struct Upload { VkBuffer buf; VkImage img; std::vector<VkBufferImageCopy> rg; };
std::unordered_map<uint64_t, std::vector<Upload>>& pending() { static std::unordered_map<uint64_t, std::vector<Upload>> m; return m; }
constexpr size_t kSampleMax = 64;

struct Block { uint32_t w, h, bytes; };

Block block_of(VkFormat f) {
    switch (f) {
    case VK_FORMAT_BC1_RGB_UNORM_BLOCK: case VK_FORMAT_BC1_RGB_SRGB_BLOCK: case VK_FORMAT_BC1_RGBA_UNORM_BLOCK:
    case VK_FORMAT_BC1_RGBA_SRGB_BLOCK: case VK_FORMAT_BC4_UNORM_BLOCK: case VK_FORMAT_BC4_SNORM_BLOCK: return {4, 4, 8};
    case VK_FORMAT_BC2_UNORM_BLOCK: case VK_FORMAT_BC2_SRGB_BLOCK: case VK_FORMAT_BC3_UNORM_BLOCK: case VK_FORMAT_BC3_SRGB_BLOCK:
    case VK_FORMAT_BC5_UNORM_BLOCK: case VK_FORMAT_BC5_SNORM_BLOCK: case VK_FORMAT_BC6H_UFLOAT_BLOCK: case VK_FORMAT_BC6H_SFLOAT_BLOCK:
    case VK_FORMAT_BC7_UNORM_BLOCK: case VK_FORMAT_BC7_SRGB_BLOCK: return {4, 4, 16};
    default: return {1, 1, texel_bytes(f)};
    }
}

// zlib's CRC-32, so gpu_tex.py can hash a .vtex_c mip the same way at C speed (zlib.crc32).
uint32_t crc32(const void* data, size_t n, uint32_t crc = 0) {
    static const auto table = [] {
        std::vector<uint32_t> t(256);
        for (uint32_t i = 0; i < 256; i++) {
            uint32_t c = i;
            for (int k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
            t[i] = c;
        }
        return t;
    }();
    auto p = (const uint8_t*)data;
    crc = ~crc;
    for (size_t i = 0; i < n; i++) crc = table[(crc ^ p[i]) & 0xff] ^ (crc >> 8);
    return ~crc;
}

// Rows are hashed one by one, seeded by their place, and summed: the result doesn't depend on how the
// engine split the mip into regions or on its row pitch.
void hash_region(Device* d, VkBuffer buf, ImageInfo& img, const VkBufferImageCopy& c) {
    Block b = block_of(img.fmt);
    if (!b.bytes || c.imageSubresource.aspectMask != VK_IMAGE_ASPECT_COLOR_BIT) return;
    uint32_t w = c.imageExtent.width, h = c.imageExtent.height, depth = std::max(1u, c.imageExtent.depth);
    uint32_t rowTexels = c.bufferRowLength ? c.bufferRowLength : w, imgRows = c.bufferImageHeight ? c.bufferImageHeight : h;
    size_t rowBytes = (size_t)((w + b.w - 1) / b.w) * b.bytes, pitch = (size_t)((rowTexels + b.w - 1) / b.w) * b.bytes;
    uint32_t rows = (h + b.h - 1) / b.h, sliceRows = (imgRows + b.h - 1) / b.h, layers = c.imageSubresource.layerCount;
    if (layers == VK_REMAINING_ARRAY_LAYERS) layers = img.layers - c.imageSubresource.baseArrayLayer;
    size_t slices = (size_t)layers * depth, span = ((slices - 1) * sliceRows + rows - 1) * pitch + rowBytes;
    auto bytes = read_buffer(d, buf, c.bufferOffset, span);
    if (bytes.empty()) return;
    uint32_t mip = c.imageSubresource.mipLevel;
    uint64_t sum = 0;
    for (size_t s = 0; s < slices; s++)
        for (uint32_t y = 0; y < rows; y++) {
            uint32_t at[4] = {c.imageSubresource.baseArrayLayer + (uint32_t)(s / depth), (uint32_t)c.imageOffset.z + (uint32_t)(s % depth),
                              (uint32_t)c.imageOffset.y / b.h + y, (uint32_t)c.imageOffset.x / b.w};
            sum += crc32(bytes.data() + (s * sliceRows + y) * pitch, rowBytes, crc32(at, sizeof at));
        }
    // A re-upload of a mip (streaming, a dynamic texture) replaces its earlier content.
    ImageContent& ic = img.content;
    if (c.imageOffset.x == 0 && c.imageOffset.y == 0 && c.imageOffset.z == 0 && c.imageSubresource.baseArrayLayer == 0) ic.mipHash[mip] = 0;
    ic.mipHash[mip] += sum;
    if (c.imageSubresource.baseArrayLayer == 0 && (ic.sampleMip == UINT32_MAX || mip >= ic.sampleMip) && rows * rowBytes <= kSampleMax) {
        ic.sampleMip = mip;
        ic.sample.clear();
        for (uint32_t y = 0; y < rows; y++) ic.sample.insert(ic.sample.end(), bytes.begin() + y * pitch, bytes.begin() + y * pitch + rowBytes);
    }
}

}  // namespace

namespace gpu {

void uploads_begin(VkCommandBuffer cb) { pending().erase(h64(cb)); }

void uploads_record(VkCommandBuffer cb, VkBuffer buf, VkImage img, std::vector<VkBufferImageCopy> rg) {
    pending()[h64(cb)].push_back({buf, img, std::move(rg)});
}

// Called before the submit reaches the driver: the host wrote the source buffers before submitting them.
void uploads_submitted(Device* d, const std::vector<VkCommandBuffer>& cbs) {
    for (auto cb : cbs) {
        auto it = pending().find(h64(cb));
        if (it == pending().end()) continue;
        for (auto& u : it->second) {
            auto img = objs().images.find(h64(u.img));
            if (img == objs().images.end()) continue;
            for (auto& c : u.rg) hash_region(d, u.buf, img->second, c);
        }
    }
}

}  // namespace gpu

static const bool g_on = getenv("UNDERBOX_GPU_DIR") && *getenv("UNDERBOX_GPU_DIR");

void hk_CmdCopyBufferToImage(VkCommandBuffer cb, VkBuffer s, VkImage d, VkImageLayout l, uint32_t n, const VkBufferImageCopy* rg) {
    if (g_on) { Lock lk(mu()); uploads_record(cb, s, d, {rg, rg + n}); }
    xfer(cb, "copyBufferToImage", h64(d));
    device(cb)->CmdCopyBufferToImage(cb, s, d, l, n, rg);
}

void hk_CmdCopyBufferToImage2(VkCommandBuffer cb, const VkCopyBufferToImageInfo2* i) {
    if (g_on) {
        std::vector<VkBufferImageCopy> rg;
        for (uint32_t k = 0; k < i->regionCount; k++) {
            auto& r = i->pRegions[k];
            rg.push_back({r.bufferOffset, r.bufferRowLength, r.bufferImageHeight, r.imageSubresource, r.imageOffset, r.imageExtent});
        }
        Lock lk(mu());
        uploads_record(cb, i->srcBuffer, i->dstImage, std::move(rg));
    }
    xfer(cb, "copyBufferToImage", h64(i->dstImage));
    device(cb)->CmdCopyBufferToImage2(cb, i);
}
