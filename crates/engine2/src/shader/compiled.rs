//! What our compiles put in a `.shader_c`, inside the framing managed reads (ShaderSource.cs,
//! ShaderUpgrade.cs): a container whose empty DATA lands on SPRV, SPRV holding one
//! `{offset, size}` header per `VFX_PROGRAM_*` and then the program bodies, an optional HLSL
//! block with the masked source as JSON. Each body starts with the VCS header managed checks
//! (`vcs2`, VCS version, ABI epoch); after it, native's layout is closed, so ours follows `UBX1`:
//! the FEATURE body is the parsed shader as JSON, a stage body its compiled combos. Bodies
//! without `UBX1` (native-compiled packages) load from their HLSL block instead (`load.rs`).

use serde::{Deserialize, Serialize};
use shaders::VfxShader;
use std::collections::{BTreeMap, HashMap};

// From ShaderUpgradeTest's `Current`: native's VFX_SHADER_RESOURCE_VERSION, VFX_VCS_VERSION_MIN_SUPPORTED,
// VFX_VCS_VERSION and VFX_ABI_CURRENT_VERSION. Unchecked against a shipped engine2.
pub const RESOURCE_VERSION: i32 = 4;
pub const MIN_VCS_VERSION: i32 = 65;
pub const VCS_VERSION: i32 = 68;
pub const ABI_VERSION: i32 = 2;

/// `VFX_PROGRAM_MAX`: FEATURE, VS, PS, GS, HS, DS, CS, PS_RENDER_STATE, RTX.
pub const PROGRAM_MAX: usize = 9;
pub const FEATURE: i64 = 0;
const VCS_MAGIC: &[u8; 4] = b"vcs2";
const OURS: &[u8; 4] = b"UBX1";
/// `CVfxShaderFileVulkan`'s SHADER_FILE_VERSION (SpirvDump.cs): no specialization list follows.
const SHADER_FILE_VERSION: u32 = 4;
const SPRV: &[u8; 4] = b"SPRV";
const HLSL: &[u8; 4] = b"HLSL";

/// `(static id, dynamic id)` → that combo's `CVfxShaderFileVulkan` blob.
pub type Combos = BTreeMap<(u64, u64), Vec<u8>>;

/// The FEATURE body: the shader as parsed, and the hash `VFX_CHECK_MD5_AGAINST_SOURCE` compares.
#[derive(Serialize, Deserialize)]
pub struct Features {
    pub source_hash: String,
    pub shader: VfxShader,
}

/// A combo's blob as managed reads it (SpirvDump.GetSpirv): version, SPIR-V size, SPIR-V.
pub fn combo_blob(spirv: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + spirv.len() * 4);
    out.extend(SHADER_FILE_VERSION.to_le_bytes());
    out.extend(((spirv.len() * 4) as i32).to_le_bytes());
    spirv.iter().for_each(|w| out.extend(w.to_le_bytes()));
    out
}

fn body(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + payload.len());
    out.extend(VCS_MAGIC);
    out.extend(VCS_VERSION.to_le_bytes());
    out.extend(ABI_VERSION.to_le_bytes());
    out.extend(OURS);
    out.extend(payload);
    out
}

pub fn features_body(f: &Features) -> Vec<u8> {
    body(&serde_json::to_vec(f).unwrap_or_default())
}

pub fn combos_body(combos: &Combos) -> Vec<u8> {
    let mut payload = (combos.len() as u32).to_le_bytes().to_vec();
    for ((s, d), blob) in combos {
        payload.extend(s.to_le_bytes());
        payload.extend(d.to_le_bytes());
        payload.extend((blob.len() as u32).to_le_bytes());
        payload.extend(blob);
    }
    body(&payload)
}

/// SPRV's payload for program bodies indexed by `VFX_PROGRAM_*` (ShaderSource.Serialize's index).
pub fn sprv_payload(bodies: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let header = PROGRAM_MAX * 8;
    let mut index = vec![0u8; header];
    let mut out: Vec<u8> = Vec::new();
    for (program, b) in bodies.iter().filter(|(p, _)| *p < PROGRAM_MAX) {
        let at = program * 8;
        index[at..at + 4].copy_from_slice(&((header + out.len()) as u32).to_le_bytes());
        index[at + 4..at + 8].copy_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend(b);
    }
    index.extend(out);
    index
}

/// What a `.shader_c` holds, as far as we read it.
#[derive(Default)]
pub struct Loaded {
    pub features: Option<Features>,
    /// Stage programs with our combo bodies.
    pub combos: HashMap<i64, Combos>,
    /// Every program with a body, ours or not.
    pub programs: Vec<i64>,
    /// The HLSL block's `{Programs: {"VFX_PROGRAM_VS": masked source}}`.
    pub embedded: HashMap<String, String>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn read_combos(p: &[u8]) -> Option<Combos> {
    let mut at = 4;
    let mut combos = Combos::new();
    for _ in 0..u32_at(p, 0)? {
        let (s, d, len) = (u64_at(p, at)?, u64_at(p, at + 8)?, u32_at(p, at + 16)? as usize);
        combos.insert((s, d), p.get(at + 20..at + 20 + len)?.to_vec());
        at += 20 + len;
    }
    Some(combos)
}

#[derive(Deserialize)]
struct Embedded {
    #[serde(rename = "Programs")]
    programs: HashMap<String, String>,
}

/// Reads a `.shader_c`; `Err` when it isn't one or a program body of ours is corrupt.
pub fn read(file: &[u8]) -> Result<Loaded, String> {
    let blocks = assets::container::Container::read(file).map_err(|e| format!("not a compiled resource: {e:?}"))?;
    let block = |kind: &[u8; 4]| blocks.blocks.iter().find(|(k, _)| k == kind).map(|(_, b)| b.as_slice());
    let sprv = block(SPRV).ok_or("no SPRV block")?;
    let mut out = Loaded::default();
    if let Some(json) = block(HLSL).filter(|b| !b.is_empty()) {
        out.embedded = serde_json::from_slice::<Embedded>(json).map_err(|e| format!("HLSL block: {e}"))?.programs;
    }
    for program in 0..PROGRAM_MAX {
        let (Some(offset), Some(size)) = (u32_at(sprv, program * 8), u32_at(sprv, program * 8 + 4)) else {
            return Err("SPRV block truncated".into());
        };
        if offset == 0 {
            continue;
        }
        let b = sprv.get(offset as usize..(offset + size) as usize).ok_or("program body out of bounds")?;
        if b.get(..4) != Some(VCS_MAGIC) {
            return Err(format!("program {program} isn't a VCS body"));
        }
        out.programs.push(program as i64);
        let Some(payload) = b.get(12..).and_then(|b| b.strip_prefix(OURS)) else { continue };
        let bad = || format!("program {program}: corrupt body");
        if program as i64 == FEATURE {
            out.features = Some(serde_json::from_slice(payload).map_err(|e| format!("{}: {e}", bad()))?);
        } else {
            out.combos.insert(program as i64, read_combos(payload).ok_or_else(bad)?);
        }
    }
    Ok(out)
}
