//! Which structured buffers the packed-buffer pass (src/packed_buffers.rs) rewrites, and which
//! padded ones it keeps and why, over sbox-public's core shaders plus the `.shader` sources under
//! `UNDERBOX_REAL_SHADERS` (`:`-separated dirs, e.g. game checkouts; read only). Prints one line
//! per buffer; fails only if sprite_cs's buffers stop being rewritten.

mod common;

use shaders::VfxShader;
use std::collections::BTreeSet;

fn audit(dir: &std::path::Path, includes: &shaders::IncludePaths, lines: &mut BTreeSet<String>) {
    for (rel, path) in common::shader_files(dir) {
        let Ok(shader) = VfxShader::parse_file(&path, includes) else { continue };
        for p in &shader.programs {
            let src = &p.source;
            for (i, _) in src.match_indices("_UbLoad( uint ub_i )") {
                let name = src[..i].rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or_default();
                lines.insert(format!("{rel}\t{name}\trewritten"));
            }
            for kept in shaders::packed_buffers::audit(src) {
                lines.insert(format!("{rel}\t{kept}"));
            }
        }
    }
}

#[test]
fn padded_structured_buffers() {
    let Some(core) = common::core_dir() else { return eprintln!("no sbox-public checkout; skipped") };
    let includes = common::includes(&core);
    let mut lines = BTreeSet::new();
    audit(&core.join("shaders"), &includes, &mut lines);
    let core_lines = lines.clone();
    for dir in std::env::var("UNDERBOX_REAL_SHADERS").unwrap_or_default().split(':').filter(|d| !d.is_empty()) {
        audit(std::path::Path::new(dir), &includes, &mut lines);
    }
    for l in &lines {
        eprintln!("{l}");
    }
    for buffer in ["SpriteBuffer", "SpriteBufferOut"] {
        assert!(core_lines.contains(&format!("sprite/sprite_cs.shader\t{buffer}\trewritten")), "{buffer}");
    }
}
