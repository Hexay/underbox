//! Just enough of a `.shader` to bind material textures: which `CreateInputTexture*` inputs feed
//! which texture attributes. Both declaration forms are read:
//!   `Texture2D g_tRma < Channel( R, Box( TextureRoughness ), Linear ); ... >;`
//!   `CreateTexture2D( g_tColor ) < Channel( RGBA, Box( TextureColor ), Srgb ); ... >;`
//! The full vfx parser (shader compilation) is separate work; this only scans declarations.

/// (input, attribute) pairs, one per input an attribute's channels read.
pub fn texture_inputs(src: &str) -> Vec<(String, String)> {
    let code = strip_comments(src);
    let inputs = declared_inputs(&code);
    let mut pairs = Vec::new();
    let mut start = 0;
    while let Some(open) = code[start..].find('<').map(|i| start + i) {
        let statement = &code[statement_start(&code, open)..open];
        let Some(end) = code[open..].find(">;").or_else(|| code[open..].find('>')).map(|i| open + i) else { break };
        start = end + 1;
        if !statement.contains("Texture") {
            continue;
        }
        let Some(attr) = identifiers(statement).filter(|w| w.starts_with("g_t")).last() else { continue };
        for input in identifiers(&code[open + 1..end]).filter(|i| inputs.contains(i)) {
            let pair = (input.to_owned(), attr.to_owned());
            if !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
    }
    pairs
}

fn strip_comments(src: &str) -> String {
    src.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// Where the statement containing `at` starts: after the previous `;`, `{` or `}`.
fn statement_start(code: &str, at: usize) -> usize {
    code[..at].rfind([';', '{', '}']).map_or(0, |i| i + 1)
}

fn declared_inputs(code: &str) -> Vec<&str> {
    code.match_indices("CreateInputTexture")
        .filter_map(|(i, _)| {
            let args = &code[i..];
            let open = args.find('(')?;
            let close = open + args[open..].find([',', ')'])?;
            Some(args[open + 1..close].trim())
        })
        .collect()
}

fn identifiers(s: &str) -> impl DoubleEndedIterator<Item = &str> {
    s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty())
}
