// Frame selection. With UNDERBOX_GPU_DIR set, the present after <dir>/trigger appears arms recording;
// the frame between the next two presents is written to <dir>/frame.jsonl, then <dir>/done.
// Recording starts UNDERBOX_GPU_LEAD (default 3) frames early so command buffers recorded ahead of
// their submit are complete (any still begun before that are listed as unrecorded) and a render-target
// dump can learn its pass (dump.cpp).
#include "json.h"
#include <cstdlib>
#include <unistd.h>

using namespace gpu;
using Lock = std::lock_guard<std::mutex>;

enum class St { Idle, Arming, Capturing, Done };
static const char* g_env = getenv("UNDERBOX_GPU_DIR");
static const std::string g_dir = g_env ? g_env : "";
static St g_st = St::Idle;
static Frame g_frame;
static int g_presents = 0;
static int g_lead = getenv("UNDERBOX_GPU_LEAD") ? std::max(1, atoi(getenv("UNDERBOX_GPU_LEAD"))) : 3;

static void learn(VkCommandBuffer cb) {
    CmdRec* r = rec(cb);
    if (!r) return;
    for (auto& e : r->ev) {
        if (e.k == Event::Pass) dump_submit_order(*e.pass, false);
        if (e.k == Event::Exec) for (auto s : e.exec) learn(s);
    }
}

static void submitted(Device* d, VkQueue q, const std::vector<VkCommandBuffer>& cbs) {
    if (!recording().load()) return;
    Lock l(mu());
    if (g_st != St::Capturing) {
        for (auto cb : cbs) learn(cb);
        bool dumps = false;
        for (auto cb : cbs) if (CmdRec* r = rec(cb)) dumps |= !r->dumps.empty();
        if (!dumps) return;
        d->QueueWaitIdle(q);
        for (auto cb : cbs) if (CmdRec* r = rec(cb)) dump_write(d, *r, nullptr, "");
        return;
    }
    // Uniform contents, indirect arguments and dumps are read once the GPU is done with the submit.
    d->QueueWaitIdle(q);
    fprintf(g_frame.f, "{\"t\":\"submit\",\"cbs\":%zu}\n", cbs.size());
    for (auto cb : cbs) {
        emit_cmdbuf(d, g_frame, cb);
        if (CmdRec* r = rec(cb)) dump_write(d, *r, g_frame.f, g_dir);
    }
}

VkResult hk_QueueSubmit(VkQueue q, uint32_t n, const VkSubmitInfo* s, VkFence f) {
    Device* d = device(q);
    VkResult r = d->QueueSubmit(q, n, s, f);
    if (recording().load() && r == VK_SUCCESS) {
        std::vector<VkCommandBuffer> cbs;
        for (uint32_t i = 0; i < n; i++) cbs.insert(cbs.end(), s[i].pCommandBuffers, s[i].pCommandBuffers + s[i].commandBufferCount);
        submitted(d, q, cbs);
    }
    return r;
}

VkResult hk_QueueSubmit2(VkQueue q, uint32_t n, const VkSubmitInfo2* s, VkFence f) {
    Device* d = device(q);
    VkResult r = d->QueueSubmit2(q, n, s, f);
    if (recording().load() && r == VK_SUCCESS) {
        std::vector<VkCommandBuffer> cbs;
        for (uint32_t i = 0; i < n; i++)
            for (uint32_t k = 0; k < s[i].commandBufferInfoCount; k++) cbs.push_back(s[i].pCommandBufferInfos[k].commandBuffer);
        submitted(d, q, cbs);
    }
    return r;
}

static void presented() {
    Lock l(mu());
    g_presents++;
    dump_new_frame();
    switch (g_st) {
    case St::Idle:
        if (access((g_dir + "/trigger").c_str(), F_OK) == 0) {
            g_st = St::Arming;
            recording() = true;
            fprintf(stderr, "gpu-capture: armed at present %d\n", g_presents);
        }
        break;
    case St::Arming:
        if (--g_lead > 0) break;
        g_frame.f = fopen((g_dir + "/frame.jsonl").c_str(), "w");
        if (!g_frame.f) {
            fprintf(stderr, "gpu-capture: failed: can't write %s/frame.jsonl\n", g_dir.c_str());
            g_st = St::Done;
            recording() = false;
            break;
        }
        g_frame.dir = g_dir;
        fprintf(g_frame.f, "{\"t\":\"frame\",\"present\":%d}\n", g_presents);
        g_st = St::Capturing;
        break;
    case St::Capturing: {
        fprintf(g_frame.f, "{\"t\":\"present\",\"passes\":%d,\"draws\":%d,\"unrecorded\":%d}\n", g_frame.pass + 1, g_frame.draws, g_frame.unrecorded);
        fclose(g_frame.f);
        g_frame.f = nullptr;
        recording() = false;
        cmds().clear();
        g_st = St::Done;
        if (FILE* f = fopen((g_dir + "/done").c_str(), "w")) fclose(f);
        fprintf(stderr, "gpu-capture: done: %d passes, %d draws, %d unrecorded command buffers\n", g_frame.pass + 1, g_frame.draws, g_frame.unrecorded);
        break;
    }
    case St::Done: break;
    }
}

VkResult hk_QueuePresentKHR(VkQueue q, const VkPresentInfoKHR* pi) {
    VkResult r = device(q)->QueuePresentKHR(q, pi);
    if (!g_dir.empty()) presented();
    return r;
}
