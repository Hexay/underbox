// Enum names through vulkan.hpp, kept in one translation unit: the header is slow to compile.
#define VULKAN_HPP_NO_EXCEPTIONS
#include "capture.h"
#include <vulkan/vulkan.hpp>
#include <vulkan/vulkan_to_string.hpp>

namespace gpu {

#define UB_NM(T) std::string nm(Vk##T v) { return vk::to_string(static_cast<vk::T>(v)); }
UB_NAMED(UB_NM)
#undef UB_NM

std::string cull_name(VkCullModeFlags c) {
    switch (c) {
    case VK_CULL_MODE_NONE: return "None";
    case VK_CULL_MODE_FRONT_BIT: return "Front";
    case VK_CULL_MODE_BACK_BIT: return "Back";
    default: return "FrontAndBack";
    }
}

}  // namespace gpu
