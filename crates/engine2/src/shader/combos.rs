//! `CVfxComboIterator` and `DescribeCombo`: a program's valid static and dynamic combo ids.

use super::{shader, stage};
use abi::slots::{c_vfx as vfx, c_vfx_combo_iterator as it};
use abi::{NativeRef, Registry, StrRet, bind, class};
use shaders::{ComboSet, INVALID_INDEX, VfxShader};
use std::sync::Mutex;

/// A program's combo spaces and the iterator's position in each. A program the shader doesn't
/// have iterates nothing.
struct Iter {
    sets: Option<(ComboSet, ComboSet)>,
    s: u64,
    d: u64,
}

/// "S_FOO=1 D_BAR=2" for the combos not at their minimum, as native's inline `DescribeCombo`.
pub(super) fn describe(s: &VfxShader, program: i64, static_id: u64, dynamic_id: u64) -> String {
    let Some(p) = stage(program).and_then(|st| s.program_for(st)) else { return String::new() };
    [(&p.statics, static_id), (&p.dynamics, dynamic_id)]
        .into_iter()
        .flat_map(|(set, id)| set.combos.iter().zip(set.decode(id)).filter(|(c, v)| *v != c.min).map(|(c, v)| format!("{}={v}", c.name)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn with_iter(h: NativeRef<class::CVfxComboIterator>, f: impl FnOnce(&mut Iter, &ComboSet, &ComboSet) -> u64) -> u64 {
    let Some(i) = objects::get::<_, Mutex<Iter>>(h) else { return INVALID_INDEX };
    let mut i = i.lock().unwrap();
    let Some((statics, dynamics)) = i.sets.clone() else { return INVALID_INDEX };
    f(&mut i, &statics, &dynamics)
}

fn next(current: u64, set: &ComboSet) -> u64 {
    if current == INVALID_INDEX { INVALID_INDEX } else { set.next_valid(current + 1) }
}

pub fn register(r: &mut Registry) {
    bind!(r, vfx::DESCRIBE_COMBO, |h, program, s, d| StrRet::from(shader(h).map(|sh| describe(&sh, program, s, d)).unwrap_or_default()));
    bind!(r, vfx::GET_ITERATOR, |h, program| {
        let p = shader(h).and_then(|s| stage(program).and_then(|st| s.program_for(st).cloned()));
        objects::create(Mutex::new(Iter { sets: p.map(|p| (p.statics, p.dynamics)), s: INVALID_INDEX, d: INVALID_INDEX }))
    });

    bind!(r, it::DELETE, |h| {
        objects::destroy(h);
    });
    bind!(r, it::INVALID_INDEX, |_| INVALID_INDEX);
    bind!(r, it::FIRST_STATIC_COMBO, |h| with_iter(h, |i, s, _| {
        i.s = s.next_valid(0);
        i.s
    }));
    bind!(r, it::NEXT_STATIC_COMBO, |h| with_iter(h, |i, s, _| {
        i.s = next(i.s, s);
        i.s
    }));
    bind!(r, it::SET_STATIC_COMBO, |h, c| with_iter(h, |i, s, _| {
        i.s = if s.is_valid(c) { c } else { INVALID_INDEX };
        i.s
    }));
    bind!(r, it::FIRST_DYNAMIC_COMBO, |h| with_iter(h, |i, _, d| {
        i.d = d.next_valid(0);
        i.d
    }));
    bind!(r, it::NEXT_DYNAMIC_COMBO, |h| with_iter(h, |i, _, d| {
        i.d = next(i.d, d);
        i.d
    }));
    bind!(r, it::SET_DYNAMIC_COMBO, |h, c| with_iter(h, |i, _, d| {
        i.d = if d.is_valid(c) { c } else { INVALID_INDEX };
        i.d
    }));
}
