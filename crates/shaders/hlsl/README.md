# HLSL shipped with crates/shaders

- `prelude.hlsl` is prepended to every program before DXC. It maps Slang-only qualifiers in the core
  shaders (`dynamic_uniform`, `asDynamicUniform`) to plain HLSL.
- `builtin/` holds our own replacements for Valve's closed core headers. The s&box core shaders include
  them, but sbox-public doesn't ship them: `math_general.fxc`, `instancing.fxc`, `ambient_cube.fxc`, and
  others. They are written from the corpus call sites and standard definitions; nothing is copied from
  Valve or Facepunch builds. An include falls back to a built-in only when it resolves nowhere on disk,
  so a real header always wins (`src/include.rs`, `src/builtin.rs`).
- A shader may define a function a built-in also defines (grubs' `SrgbGammaToLinear`). When the
  signatures match, the built-in's body is dropped and its definition kept as a prototype
  (`src/overrides.rs`), so a built-in never needs `#ifndef` guards for that.

Engine-data headers (instancing, light probes, lightmaps, volumetric fog) define resource layouts. The
renderer fills those resources, so the layout chosen here is the contract between the two, and the
renderer must follow it. Each such header marks its open questions with `TODO`.
