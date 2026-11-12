// Force-included in every Steam Audio source: the standard part of its pch.h, which its CMake
// build precompiles into each file. Its SOFA, IPP and Windows.h parts stay out (unused here).
#pragma once

#include <stdlib.h>
#include <string.h>

#ifdef __cplusplus
#include <algorithm>
#include <atomic>
#include <complex>
#include <cstdarg>
#include <cstdint>
#include <cstring>
#include <functional>
#include <iterator>
#include <memory>
#include <mutex>
#include <new>
#include <numeric>
#include <vector>
#endif
