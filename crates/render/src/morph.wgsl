// Morph composite quads (morph.rs): each texel of a quad is one mesh vertex, `y * width + x` from
// the quad's corner, and gets that vertex's delta from the source times the quad's weight.

@group(0) @binding(0) var<storage, read> deltas: array<vec4<f32>>;

struct In {
    @location(0) position: vec2<f32>,
    @location(1) cell: vec2<f32>,
    // first float4 of the target in `deltas`, vertex count, region width, weight
    @location(2) params: vec4<f32>,
}

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) cell: vec2<f32>,
    @location(1) @interpolate(flat) params: vec4<f32>,
}

@vertex
fn vs_main(i: In) -> Out {
    var o: Out;
    o.position = vec4<f32>(i.position, 0.0, 1.0);
    o.cell = i.cell;
    o.params = i.params;
    return o;
}

@fragment
fn fs_main(v: Out) -> @location(0) vec4<f32> {
    let c = vec2<u32>(floor(v.cell));
    let vertex = c.y * u32(v.params.z) + c.x;
    if vertex >= u32(v.params.y) {
        return vec4<f32>(0.0);
    }
    return deltas[u32(v.params.x) + vertex] * v.params.w;
}
