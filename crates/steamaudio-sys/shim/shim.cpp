// C entry points over Steam Audio's internal binaural effect and HRTF database, plus the two globals
// its context.cpp would define (that file drags in the whole API layer).

#include "binaural_effect.h"
#include "log.h"
#include "memory_allocator.h"

namespace ipl {
static Log sLog;
static Memory sMemory;
Log& gLog() { return sLog; }
Memory& gMemory() { return sMemory; }
}

using namespace ipl;

extern "C" {

HRTFDatabase* ubx_sa_hrtf_create(int rate, int frame)
{
    try
    {
        HRTFSettings settings{};
        // Real engine2's choice: each HRIR is scaled to the loudness of the one straight ahead.
        settings.normType = HRTFNormType::RMS;
        return new HRTFDatabase(settings, rate, frame);
    }
    catch (...)
    {
        return nullptr;
    }
}

void ubx_sa_hrtf_destroy(HRTFDatabase* hrtf) { delete hrtf; }

BinauralEffect* ubx_sa_binaural_create(const HRTFDatabase* hrtf, int rate, int frame)
{
    try
    {
        AudioSettings audio{};
        audio.samplingRate = rate;
        audio.frameSize = frame;
        BinauralEffectSettings settings{};
        settings.hrtf = hrtf;
        return new BinauralEffect(audio, settings);
    }
    catch (...)
    {
        return nullptr;
    }
}

void ubx_sa_binaural_destroy(BinauralEffect* effect) { delete effect; }

// `direction` in Steam Audio's listener space (+x right, +y up, -z ahead). `in` has 1 or 2 channels
// of `frame` samples, `out` 2. Returns 0 on failure (out left as is).
int ubx_sa_binaural_apply(BinauralEffect* effect, const HRTFDatabase* hrtf, const float* direction, int bilinear,
                          float spatial_blend, int in_channels, float* const* in, int frame, float* const* out)
{
    try
    {
        Vector3f dir(direction[0], direction[1], direction[2]);
        BinauralEffectParams params{};
        params.direction = &dir;
        params.interpolation = bilinear ? HRTFInterpolation::Bilinear : HRTFInterpolation::NearestNeighbor;
        params.spatialBlend = spatial_blend;
        params.hrtf = hrtf;
        AudioBuffer input(in_channels, frame, in);
        AudioBuffer output(2, frame, out);
        effect->apply(params, input, output);
        return 1;
    }
    catch (...)
    {
        return 0;
    }
}
}
