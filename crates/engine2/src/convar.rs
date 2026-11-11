//! Native `ConVar`s: engine variables managed wraps as `NativeConVar` once native announces them
//! (`ConVarSystem.RegisterNativeVar`), then reads and sets through these slots.
//! Subsystems [`define`] theirs (render/convars.rs); `SourceEngineInit` calls [`announce_all`].

use abi::callbacks::sandbox_con_var_system as managed;
use abi::slots::con_var as cv;
use abi::{NativeRef, Registry, StrRet, bind, class};
use std::ffi::{CStr, CString};
use std::sync::{Mutex, RwLock};

pub type Ref = NativeRef<class::ConVar>;

/// `ConVarFlags_t` (Sandbox.Engine/Core/Interop/NativeEngine/CommandFlags.cs).
pub const FCVAR_ARCHIVE: i64 = 1 << 7;

#[derive(Debug, Clone, Default)]
pub struct Spec {
    pub name: String,
    pub default: String,
    pub help: String,
    pub flags: i64,
    pub min: Option<f32>,
    pub max: Option<f32>,
}

pub struct ConVar {
    pub spec: Spec,
    value: Mutex<String>,
}

impl ConVar {
    pub fn new(spec: Spec) -> Self {
        let value = Mutex::new(spec.default.clone());
        Self { spec, value }
    }

    pub fn get(&self) -> String {
        self.value.lock().unwrap().clone()
    }

    /// `atoi`-like: leading integer, else a float truncated, else 0.
    pub fn get_int(&self) -> i32 {
        let v = self.get();
        let v = v.trim();
        v.parse::<i32>().ok().or_else(|| v.parse::<f32>().ok().map(|f| f as i32)).unwrap_or(0)
    }

    /// Store `value`, clamped to the bounds when numeric; the previous value if it changed.
    pub fn set(&self, value: &str) -> Option<String> {
        let value = match value.trim().parse::<f32>() {
            Ok(f) if self.spec.min.is_some_and(|m| f < m) || self.spec.max.is_some_and(|m| f > m) => {
                let f = f.clamp(self.spec.min.unwrap_or(f32::MIN), self.spec.max.unwrap_or(f32::MAX));
                format_float(f)
            }
            _ => value.to_owned(),
        };
        let previous = std::mem::replace(&mut *self.value.lock().unwrap(), value.clone());
        (previous != value).then_some(previous)
    }
}

/// Whole floats print without a fraction, as `ConVar::SetValue(float)` stores them.
fn format_float(f: f32) -> String {
    if f.fract() == 0.0 && f.abs() < 1e9 { format!("{}", f as i64) } else { f.to_string() }
}

static DEFINED: RwLock<Vec<Ref>> = RwLock::new(Vec::new());

/// Make an engine convar; managed sees it after [`announce_all`].
pub fn define(spec: Spec) -> Ref {
    let r = objects::create(ConVar::new(spec));
    DEFINED.write().unwrap().push(r);
    r
}

pub fn find(name: &str) -> Option<std::sync::Arc<ConVar>> {
    DEFINED.read().unwrap().iter().filter_map(|&r| get(r)).find(|c| c.spec.name.eq_ignore_ascii_case(name))
}

/// Hand every defined convar to managed's ConVarSystem. Calls managed: not from a `[nogc]` slot.
pub fn announce_all() {
    for &r in DEFINED.read().unwrap().iter() {
        managed::register_native_var(r);
    }
}

fn get(r: Ref) -> Option<std::sync::Arc<ConVar>> {
    objects::get(r)
}

fn read<R: Default>(r: Ref, f: impl FnOnce(&ConVar) -> R) -> R {
    get(r).map(|c| f(&c)).unwrap_or_default()
}

fn set(r: Ref, value: &str) {
    let Some(c) = get(r) else { return };
    if let Some(previous) = c.set(value) {
        let cstr = |s: String| CString::new(s.replace('\0', "")).unwrap_or_default();
        let (name, value, previous) = (cstr(c.spec.name.clone()), cstr(c.get()), cstr(previous));
        managed::on_con_var_changed(Some(&name), Some(&value), Some(&previous));
    }
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, cv::GET_NAME, |r| StrRet::from(read(r, |c| c.spec.name.clone())));
    bind!(r, cv::GET_HELP_TEXT, |r| StrRet::from(read(r, |c| c.spec.help.clone())));
    bind!(r, cv::GET_DEFAULT, |r| StrRet::from(read(r, |c| c.spec.default.clone())));
    bind!(r, cv::GET_STRING, |r| StrRet::from(read(r, ConVar::get)));
    bind!(r, cv::GET_INT, |r| read(r, ConVar::get_int));
    bind!(r, cv::GET_FLAGS, |r| read(r, |c| c.spec.flags));
    bind!(r, cv::HAS_MIN, |r| read(r, |c| c.spec.min.is_some()));
    bind!(r, cv::HAS_MAX, |r| read(r, |c| c.spec.max.is_some()));
    bind!(r, cv::GET_MIN_VALUE, |r| read(r, |c| c.spec.min.unwrap_or_default()));
    bind!(r, cv::GET_MAX_VALUE, |r| read(r, |c| c.spec.max.unwrap_or_default()));
    bind!(r, cv::SET_VALUE, |r, v| set(r, &text(v)));
    bind!(r, cv::SET_VALUE_1, |r, v: f32| set(r, &format_float(v)));
    bind!(r, cv::SET_VALUE_2, |r, v: i32| set(r, &v.to_string()));
    bind!(r, cv::REVERT, |r| {
        if let Some(c) = get(r) {
            set(r, &c.spec.default.clone());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fps_max() -> ConVar {
        ConVar::new(Spec { name: "fps_max".into(), default: "300".into(), flags: FCVAR_ARCHIVE, min: Some(0.0), max: Some(1000.0), ..Spec::default() })
    }

    #[test]
    fn values_clamp_and_report_changes() {
        let c = fps_max();
        assert_eq!((c.get(), c.get_int()), ("300".to_string(), 300));
        assert_eq!(c.set("144"), Some("300".into()));
        assert_eq!(c.set("144"), None, "unchanged");
        assert_eq!(c.set("5000"), Some("144".into()));
        assert_eq!(c.get(), "1000", "clamped to max");
        c.set("-3");
        assert_eq!(c.get(), "0");
        c.set("59.9");
        assert_eq!(c.get_int(), 59);
        c.set("abc");
        assert_eq!((c.get(), c.get_int()), ("abc".to_string(), 0), "non-numbers are stored as is");
    }

    #[test]
    fn defined_convars_are_found_by_name() {
        let r = define(Spec { name: "r_3d_skybox".into(), default: "1".into(), ..Spec::default() });
        assert_eq!(find("R_3D_SKYBOX").map(|c| c.get_int()), Some(1));
        assert_eq!(read(r, |c| c.spec.flags), 0);
        assert!(find("missing").is_none());
    }
}
