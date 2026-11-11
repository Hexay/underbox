//! The preprocessor skeleton of a program's text: its conditionals, `#define`s and `#undef`s with
//! their lines, so what is live for given feature values can be replayed without the source
//! (engine2's `IsUiVariableActive`).

const KEPT: [&str; 8] = ["if", "ifdef", "ifndef", "elif", "else", "endif", "define", "undef"];

/// `(1-based line, directive)` from comment-free `text`, continuations joined and `#` tightened
/// (`# if X` becomes `#if X`). Function-like macros keep only `#define NAME()`: their bodies can't
/// be evaluated in a condition anyway.
pub fn collect(text: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        let Some(rest) = line.trim_start().strip_prefix('#') else { continue };
        let mut full = rest.trim_start().to_string();
        while full.ends_with('\\') {
            full.pop();
            match lines.next() {
                Some((_, next)) => full.push_str(next),
                None => break,
            }
        }
        let word = full.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').next().unwrap_or("");
        if !KEPT.contains(&word) {
            continue;
        }
        let mut directive = format!("#{}", full.trim_end());
        if word == "define" {
            let name_end = full[6..].trim_start();
            let ident = name_end.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(name_end.len());
            if name_end[ident..].starts_with('(') {
                directive = format!("#define {}()", &name_end[..ident]);
            }
        }
        out.push((n as u32 + 1, directive));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_conditionals_and_object_macros() {
        let src = "#ifndef X\n  # define X ActiveIf( F_A );\n#endif\n#define F(a) \\\n  a + 1\n#include \"y.fxc\"\n#if A && \\\n  B\nint z;\n#endif\n";
        let got = super::collect(src);
        let want = [(1, "#ifndef X"), (2, "#define X ActiveIf( F_A );"), (3, "#endif"), (4, "#define F()"), (7, "#if A &&   B"), (10, "#endif")];
        assert_eq!(got, want.map(|(l, d)| (l, d.to_string())));
    }
}
