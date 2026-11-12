// Loader plumbing: instance and device chains, proc-addr lookup, the per-device next-function table.
#include "layer.h"
#include <cstdlib>
#include <cstring>

namespace gpu {

std::mutex& mu() { static std::mutex m; return m; }
Objects& objs() { static Objects o; return o; }

uint64_t fnv(const void* data, size_t n, uint64_t h) {
    auto p = (const uint8_t*)data;
    for (size_t i = 0; i < n; i++) h = (h ^ p[i]) * 1099511628211ull;
    return h;
}

std::string hex(uint64_t v) {
    char b[17];
    snprintf(b, sizeof b, "%016llx", (unsigned long long)v);
    return b;
}

// Few devices and instances per process: fixed arrays keep lookups lock-free on the hot path.
static Device* g_devs[16];
static std::atomic<int> g_ndev{0};
struct Inst { void* key; PFN_vkGetInstanceProcAddr gipa; VkInstance inst; };
static Inst g_insts[16];
static std::atomic<int> g_ninst{0};

Device* device(const void* d) {
    void* k = key(d);
    for (int i = 0, n = g_ndev.load(); i < n; i++)
        if (g_devs[i] && g_devs[i]->key == k) return g_devs[i];
    return nullptr;
}

static Inst* instance(const void* d) {
    void* k = key(d);
    for (int i = 0, n = g_ninst.load(); i < n; i++)
        if (g_insts[i].key == k) return &g_insts[i];
    return nullptr;
}

}  // namespace gpu

using namespace gpu;

static const std::unordered_map<std::string, PFN_vkVoidFunction>& hook_table() {
    static const auto table = [] {
        std::unordered_map<std::string, PFN_vkVoidFunction> t;
#define UB_ENTRY(n) for (const char* s : {"", "KHR", "EXT"}) t[std::string("vk" #n) + s] = (PFN_vkVoidFunction)&hk_##n;
        UB_HOOKS(UB_ENTRY)
#undef UB_ENTRY
        return t;
    }();
    return table;
}

static PFN_vkVoidFunction resolve(PFN_vkGetDeviceProcAddr gdpa, VkDevice dev, const char* core) {
    for (const char* s : {"", "KHR", "EXT"})
        if (auto f = gdpa(dev, (std::string(core) + s).c_str())) return f;
    return nullptr;
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL ub_GetDeviceProcAddr(VkDevice dev, const char* name);

static VKAPI_ATTR VkResult VKAPI_CALL ub_CreateInstance(const VkInstanceCreateInfo* ci, const VkAllocationCallbacks* a, VkInstance* out) {
    auto chain = (VkLayerInstanceCreateInfo*)ci->pNext;
    while (chain && !(chain->sType == VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO && chain->function == VK_LAYER_LINK_INFO))
        chain = (VkLayerInstanceCreateInfo*)chain->pNext;
    if (!chain) return VK_ERROR_INITIALIZATION_FAILED;
    PFN_vkGetInstanceProcAddr next = chain->u.pLayerInfo->pfnNextGetInstanceProcAddr;
    chain->u.pLayerInfo = chain->u.pLayerInfo->pNext;
    auto create = (PFN_vkCreateInstance)next(VK_NULL_HANDLE, "vkCreateInstance");
    // Debug utils on, so apps that check for it label passes and name objects.
    std::vector<const char*> exts(ci->ppEnabledExtensionNames, ci->ppEnabledExtensionNames + ci->enabledExtensionCount);
    bool has = false;
    for (auto e : exts) has |= strcmp(e, VK_EXT_DEBUG_UTILS_EXTENSION_NAME) == 0;
    VkResult r = VK_ERROR_EXTENSION_NOT_PRESENT;
    if (!has && !getenv("UNDERBOX_GPU_NO_DEBUG_UTILS")) {
        exts.push_back(VK_EXT_DEBUG_UTILS_EXTENSION_NAME);
        VkInstanceCreateInfo c2 = *ci;
        c2.enabledExtensionCount = (uint32_t)exts.size();
        c2.ppEnabledExtensionNames = exts.data();
        r = create(&c2, a, out);
    }
    if (r == VK_ERROR_EXTENSION_NOT_PRESENT) r = create(ci, a, out);
    if (r != VK_SUCCESS) return r;
    int i = g_ninst.load();
    if (i < 16) { g_insts[i] = {key(*out), next, *out}; g_ninst.store(i + 1); }
    return r;
}

static VKAPI_ATTR void VKAPI_CALL ub_DestroyInstance(VkInstance inst, const VkAllocationCallbacks* a) {
    Inst* in = instance(inst);
    if (!in) return;
    auto destroy = (PFN_vkDestroyInstance)in->gipa(inst, "vkDestroyInstance");
    in->key = nullptr;
    destroy(inst, a);
}

static VKAPI_ATTR VkResult VKAPI_CALL ub_CreateDevice(VkPhysicalDevice phys, const VkDeviceCreateInfo* ci, const VkAllocationCallbacks* a, VkDevice* out) {
    auto chain = (VkLayerDeviceCreateInfo*)ci->pNext;
    while (chain && !(chain->sType == VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO && chain->function == VK_LAYER_LINK_INFO))
        chain = (VkLayerDeviceCreateInfo*)chain->pNext;
    if (!chain) return VK_ERROR_INITIALIZATION_FAILED;
    PFN_vkGetInstanceProcAddr gipa = chain->u.pLayerInfo->pfnNextGetInstanceProcAddr;
    PFN_vkGetDeviceProcAddr gdpa = chain->u.pLayerInfo->pfnNextGetDeviceProcAddr;
    chain->u.pLayerInfo = chain->u.pLayerInfo->pNext;
    Inst* in = instance(phys);
    VkInstance inst = in ? in->inst : VK_NULL_HANDLE;
    auto create = (PFN_vkCreateDevice)gipa(inst, "vkCreateDevice");
    VkResult r = create(phys, ci, a, out);
    if (r != VK_SUCCESS) return r;
    auto d = new Device;
    d->key = key(*out);
    d->dev = *out;
    d->gdpa = gdpa;
    if (auto props = (PFN_vkGetPhysicalDeviceMemoryProperties)gipa(inst, "vkGetPhysicalDeviceMemoryProperties")) props(phys, &d->mem);
#define UB_RESOLVE(n) d->n = (PFN_vk##n)resolve(gdpa, *out, "vk" #n);
    UB_NEXT(UB_RESOLVE)
#undef UB_RESOLVE
    int i = g_ndev.load();
    if (i < 16) { g_devs[i] = d; g_ndev.store(i + 1); }
    return r;
}

VKAPI_ATTR void VKAPI_CALL hk_DestroyDevice(VkDevice dev, const VkAllocationCallbacks* a) {
    Device* d = device(dev);
    if (!d) return;
    auto destroy = d->DestroyDevice;
    d->key = nullptr;
    destroy(dev, a);
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL ub_GetDeviceProcAddr(VkDevice dev, const char* name) {
    if (!strcmp(name, "vkGetDeviceProcAddr")) return (PFN_vkVoidFunction)&ub_GetDeviceProcAddr;
    Device* d = device(dev);
    if (!d) return nullptr;
    PFN_vkVoidFunction next = d->gdpa(dev, name);
    if (!next) return nullptr;
    auto& t = hook_table();
    auto it = t.find(name);
    return it == t.end() ? next : it->second;
}

static VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL ub_GetInstanceProcAddr(VkInstance inst, const char* name) {
    if (!strcmp(name, "vkGetInstanceProcAddr")) return (PFN_vkVoidFunction)&ub_GetInstanceProcAddr;
    if (!strcmp(name, "vkCreateInstance")) return (PFN_vkVoidFunction)&ub_CreateInstance;
    if (!strcmp(name, "vkGetDeviceProcAddr")) return (PFN_vkVoidFunction)&ub_GetDeviceProcAddr;
    if (!inst) return nullptr;
    if (!strcmp(name, "vkDestroyInstance")) return (PFN_vkVoidFunction)&ub_DestroyInstance;
    if (!strcmp(name, "vkCreateDevice")) return (PFN_vkVoidFunction)&ub_CreateDevice;
    Inst* in = instance(inst);
    if (!in) return nullptr;
    PFN_vkVoidFunction next = in->gipa(inst, name);
    if (!next) return nullptr;
    auto& t = hook_table();
    auto it = t.find(name);
    return it == t.end() ? next : it->second;
}

extern "C" __attribute__((visibility("default"))) VKAPI_ATTR VkResult VKAPI_CALL
vkNegotiateLoaderLayerInterfaceVersion(VkNegotiateLayerInterface* v) {
    if (v->loaderLayerInterfaceVersion > 2) v->loaderLayerInterfaceVersion = 2;
    v->pfnGetInstanceProcAddr = &ub_GetInstanceProcAddr;
    v->pfnGetDeviceProcAddr = &ub_GetDeviceProcAddr;
    v->pfnGetPhysicalDeviceProcAddr = nullptr;
    return VK_SUCCESS;
}
