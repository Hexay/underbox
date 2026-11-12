// Minimal JSON building for the capture's output lines.
#pragma once
#include "capture.h"
#include <cstdio>

namespace gpu {

inline std::string q(const std::string& s) {
    std::string o = "\"";
    for (unsigned char c : s) {
        if (c == '"' || c == '\\') { o += '\\'; o += (char)c; }
        else if (c < 0x20) { char b[8]; snprintf(b, sizeof b, "\\u%04x", c); o += b; }
        else o += (char)c;
    }
    return o + "\"";
}

inline std::string num(double v) {
    char b[32];
    snprintf(b, sizeof b, "%.9g", v);
    return b;
}

struct Obj {
    std::string s = "{";
    Obj& raw(const char* k, const std::string& v) {
        if (s.size() > 1) s += ',';
        s += '"';
        s += k;
        s += "\":";
        s += v;
        return *this;
    }
    Obj& str(const char* k, const std::string& v) { return raw(k, q(v)); }
    Obj& n(const char* k, double v) { return raw(k, num(v)); }
    std::string done() const { return s + "}"; }
};

inline std::string arr(const std::vector<std::string>& v) {
    std::string s = "[";
    for (size_t i = 0; i < v.size(); i++) s += (i ? "," : "") + v[i];
    return s + "]";
}

inline std::string hexbytes(const uint8_t* p, size_t n) {
    static const char* d = "0123456789abcdef";
    std::string s;
    s.reserve(n * 2);
    for (size_t i = 0; i < n; i++) { s += d[p[i] >> 4]; s += d[p[i] & 15]; }
    return s;
}

// describe.cpp
std::string image_obj(uint64_t image);
std::string image_short(uint64_t image);
std::string view_obj(VkImageView v);
std::string sampler_obj(VkSampler s);
std::string state_obj(const PipeInfo& p, const Dyn& y);
std::string pipe_obj(const PipeInfo& p, int id, uint64_t handle);
std::string buffer_obj(uint64_t buffer);

}  // namespace gpu
