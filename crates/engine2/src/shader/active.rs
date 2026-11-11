//! Which material variables the editor shows for a set of feature values (`IsUiVariableActive`,
//! native's `ComputeUiVariableListAndFeatureVariableIndices`).
//!
//! Each program's preprocessor skeleton is replayed with the defines a compile would get
//! (`PROGRAM`, `VULKAN`, every feature, then the combos: feature-driven ones take their feature's
//! value, the rest their minimum, i.e. the default combo). A variable is shown when its
//! declaration is live and every `ActiveIf( expr )` on it holds (also through a macro, like
//! `BENT_NORMAL_ACTIVE_IF`); an input texture when a shown texture's `Channel(...)` reads it, or
//! when no texture reads it at all. Names that are no macro, feature or combo are 0, as in the C
//! preprocessor, so `#if D_X` and unknown names hide what they guard.

use super::expr::{self, Scope};
use shaders::annotation::Annotated;
use shaders::{Program, VfxShader};
use std::collections::{HashMap, HashSet};

const MAX_DEPTH: u32 = 16;

enum Macro {
    Object(String),
    Function,
}

struct Macros(HashMap<String, Macro>);

impl Scope for Macros {
    fn defined(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    fn value(&self, name: &str, depth: u32) -> i64 {
        match self.0.get(name) {
            Some(Macro::Object(body)) if depth < MAX_DEPTH => expr::eval(body, self, depth + 1),
            _ => 0,
        }
    }
}

struct Frame {
    parent: bool,
    taken: bool,
    live: bool,
}

struct Replay {
    macros: Macros,
    stack: Vec<Frame>,
}

fn first_ident(s: &str) -> &str {
    let s = s.trim_start();
    &s[..s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len())]
}

impl Replay {
    fn live(&self) -> bool {
        self.stack.last().is_none_or(|f| f.live)
    }

    fn holds(&self, cond: &str) -> bool {
        expr::eval(cond, &self.macros, 0) != 0
    }

    /// One `directives::collect` entry.
    fn apply(&mut self, directive: &str) {
        let body = directive.trim_start_matches('#');
        let word = first_ident(body);
        let rest = &body[word.len()..];
        match word {
            "if" | "ifdef" | "ifndef" => {
                let parent = self.live();
                let live = parent
                    && match word {
                        "if" => self.holds(rest),
                        "ifdef" => self.macros.defined(first_ident(rest)),
                        _ => !self.macros.defined(first_ident(rest)),
                    };
                self.stack.push(Frame { parent, taken: live, live });
            }
            "elif" => {
                let live = self.stack.last().is_some_and(|f| f.parent && !f.taken) && self.holds(rest);
                if let Some(f) = self.stack.last_mut() {
                    f.live = live;
                    f.taken |= live;
                }
            }
            "else" => {
                if let Some(f) = self.stack.last_mut() {
                    f.live = f.parent && !f.taken;
                    f.taken = true;
                }
            }
            "endif" => {
                self.stack.pop();
            }
            "define" if self.live() => {
                let name = first_ident(rest);
                let after = &rest.trim_start()[name.len()..];
                let m = if after.starts_with('(') { Macro::Function } else { Macro::Object(after.trim().to_string()) };
                self.macros.0.insert(name.to_string(), m);
            }
            "undef" if self.live() => {
                self.macros.0.remove(first_ident(rest));
            }
            _ => {}
        }
    }
}

/// The defines a compile of `p` gets for `features` (see compiler.rs `defines`).
fn defines(p: &Program, features: &HashMap<&str, i64>) -> Macros {
    let mut m = HashMap::from([("PROGRAM".to_string(), (p.stage as i64).to_string()), ("VULKAN".into(), "1".into())]);
    m.extend(features.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    for c in p.statics.combos.iter().chain(&p.dynamics.combos) {
        let v = c.feature.as_deref().and_then(|f| features.get(f).copied()).unwrap_or(c.min as i64);
        m.insert(c.name.clone(), v.to_string());
    }
    Macros(m.into_iter().map(|(k, v)| (k, Macro::Object(v))).collect())
}

/// Replays `p` and calls `at(i, replay)` for each live `lines[i]`, in line order.
fn replay(p: &Program, macros: Macros, lines: &[u32], mut at: impl FnMut(usize, &Replay)) {
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by_key(|&i| lines[i]);
    let mut probes = order.into_iter().peekable();
    let mut r = Replay { macros, stack: Vec::new() };
    for (line, directive) in &p.directives {
        while let Some(i) = probes.next_if(|&i| lines[i] < *line) {
            if r.live() {
                at(i, &r);
            }
        }
        r.apply(directive);
    }
    for i in probes {
        if r.live() {
            at(i, &r);
        }
    }
}

/// The arguments of every `name( ... )` call in `text`.
fn call_args<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(n) = text[from..].find(name) {
        let open = from + n + name.len();
        from = open;
        let Some(args) = text[open..].trim_start().strip_prefix('(') else { continue };
        let mut depth = 1;
        if let Some(end) = args.find(|c| {
            depth += match c {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            depth == 0
        }) {
            out.push(&args[..end]);
        }
    }
    out
}

fn active_if(a: &Annotated, r: &Replay) -> bool {
    a.items.iter().all(|(key, args)| match (key.as_str(), r.macros.0.get(key)) {
        ("ActiveIf", _) => r.holds(args),
        (_, Some(Macro::Object(body))) if args.is_empty() => call_args(body, "ActiveIf").into_iter().all(|c| r.holds(c)),
        _ => true,
    })
}

fn channel_inputs(a: &Annotated) -> impl Iterator<Item = &str> {
    a.items.iter().filter(|(k, _)| k == "Channel").flat_map(|(_, args)| args.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))).filter(|w| !w.is_empty())
}

/// The names of the variables and input textures shown for `features` (every feature's value).
pub(super) fn shown(s: &VfxShader, features: &HashMap<&str, i64>) -> HashSet<String> {
    let mut shown = HashSet::new();
    for p in &s.programs {
        let annotations: Vec<&Annotated> = p.annotations.iter().chain(&p.macro_annotations).collect();
        let input_line = |i: usize| p.decls.input_lines.get(i).copied().unwrap_or(0);
        let lines: Vec<u32> = annotations.iter().map(|a| a.line).chain((0..p.decls.inputs.len()).map(input_line)).collect();
        let mut live = vec![false; lines.len()];
        replay(p, defines(p, features), &lines, |i, r| live[i] = annotations.get(i).is_none_or(|a| active_if(a, r)));
        let (mut fed, mut read) = (HashSet::new(), HashSet::new());
        for (a, &on) in annotations.iter().zip(&live) {
            if on {
                shown.insert(a.name.clone());
                fed.extend(channel_inputs(a));
            }
            read.extend(channel_inputs(a));
        }
        for (input, &on) in p.decls.inputs.iter().zip(&live[annotations.len()..]) {
            let name = input.get(1).map_or("", |n| n.trim());
            if on && (fed.contains(name) || !read.contains(name)) {
                shown.insert(name.to_string());
            }
        }
    }
    shown
}
