//! `#include "x"` expansion with the same search order as managed `ShaderPreprocessor` (relative to
//! the including file, then each content root, then `<root>/shaders/`) and plain C semantics:
//! a file is pasted every time it is included. The one shortcut is a file wrapped in an include
//! guard that was already pasted, which expands to nothing either way.

use std::path::{Path, PathBuf};

/// Where includes are searched: content roots such as `game/core` (holding `shaders/`).
#[derive(Clone, Debug, Default)]
pub struct IncludePaths {
    pub roots: Vec<PathBuf>,
}

impl IncludePaths {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        IncludePaths { roots: roots.into_iter().collect() }
    }

    pub fn resolve(&self, name: &str, from: &Path) -> Option<PathBuf> {
        let name = &name.replace('\\', "/");
        let parent = from.parent().map(Path::to_path_buf).unwrap_or_default();
        std::iter::once(parent.join(name))
            .chain(self.roots.iter().flat_map(|r| [r.join(name), r.join("shaders").join(name)]))
            .find(|p| p.is_file())
    }
}

/// One file read during expansion, with its content hash (for cache keys).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub path: PathBuf,
    pub hash: blake3::Hash,
}

pub struct Expander<'a> {
    paths: &'a IncludePaths,
    guards_done: Vec<PathBuf>,
    pub deps: Vec<Dependency>,
    pub missing: Vec<String>,
}

const MAX_DEPTH: usize = 64;

impl<'a> Expander<'a> {
    pub fn new(paths: &'a IncludePaths) -> Self {
        Expander { paths, guards_done: Vec::new(), deps: Vec::new(), missing: Vec::new() }
    }

    /// Expands `text`, which starts at `line` of `file`, emitting `#line` markers so compiler
    /// errors point at the original files. Unresolved includes are dropped and listed in `missing`.
    pub fn expand(&mut self, text: &str, file: &Path, line: u32) -> Result<String, String> {
        let mut out = String::with_capacity(text.len());
        self.expand_into(text, file, line, 0, &mut out)?;
        Ok(out)
    }

    fn expand_into(&mut self, text: &str, file: &Path, line: u32, depth: usize, out: &mut String) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("{}: includes nested deeper than {MAX_DEPTH}", file.display()));
        }
        line_marker(out, line, file);
        for (n, l) in text.lines().enumerate() {
            let Some(name) = include_target(l) else {
                out.push_str(l);
                out.push('\n');
                continue;
            };
            match self.paths.resolve(name, file) {
                Some(path) => {
                    self.paste(&path, depth, out)?;
                    line_marker(out, line + n as u32 + 1, file);
                }
                None => {
                    if !self.missing.iter().any(|m| m == name) {
                        self.missing.push(name.into());
                    }
                    out.push('\n');
                }
            }
        }
        Ok(())
    }

    fn paste(&mut self, path: &Path, depth: usize, out: &mut String) -> Result<(), String> {
        if self.guards_done.iter().any(|p| p == path) {
            return Ok(());
        }
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !self.deps.iter().any(|d| d.path == path) {
            self.deps.push(Dependency { path: path.into(), hash: blake3::hash(src.as_bytes()) });
        }
        if has_include_guard(&src) {
            self.guards_done.push(path.into());
        }
        self.expand_into(&src, path, 1, depth + 1, out)
    }
}

fn line_marker(out: &mut String, line: u32, file: &Path) {
    let f = file.to_string_lossy().replace('\\', "/");
    out.push_str(&format!("#line {line} \"{f}\"\n"));
}

/// `name` from a `#include "name"` line.
fn include_target(line: &str) -> Option<&str> {
    let t = line.trim_start().strip_prefix('#')?.trim_start().strip_prefix("include")?.trim();
    let t = t.strip_prefix('"')?;
    Some(&t[..t.find('"')?])
}

/// Whether the file's first directives are `#ifndef G` / `#define G` and its last is `#endif`.
fn has_include_guard(src: &str) -> bool {
    let src = crate::scan::blank_comments(src);
    let mut directives = src.lines().map(str::trim).filter(|l| !l.is_empty());
    let guard = directives.next().and_then(|l| l.strip_prefix("#ifndef")).map(str::trim);
    let define = directives.next().and_then(|l| l.strip_prefix("#define")).map(str::trim);
    let last = src.lines().map(str::trim).rfind(|l| !l.is_empty());
    matches!((guard, define), (Some(g), Some(d)) if !g.is_empty() && d == g) && last.is_some_and(|l| l.starts_with("#endif"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_and_guards() {
        assert_eq!(include_target("  #include \"common/a.hlsl\" // x"), Some("common/a.hlsl"));
        assert_eq!(include_target("# include \"b\""), Some("b"));
        assert_eq!(include_target("#include <c>"), None);
        assert!(has_include_guard("// c\n#ifndef A_H\n#define A_H\nint x;\n#endif // A_H\n"));
        assert!(!has_include_guard("#ifndef A_H\n#define B_H\n#endif\n"));
        assert!(!has_include_guard("#ifndef A_H\n#define A_H\n#endif\nint after;\n"));
    }
}
