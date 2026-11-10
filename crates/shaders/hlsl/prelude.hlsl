// Prepended to every program before DXC. The core shaders are written for Slang as well; these
// map its uniformity qualifiers (Bindless.hlsl) to plain HLSL, where they are only hints.
#define dynamic_uniform
#define asDynamicUniform( x ) ( x )
