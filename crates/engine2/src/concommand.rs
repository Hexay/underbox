//! Native `ConCommand`s: engine commands managed wraps as `NativeCommand` once announced
//! (`ConVarSystem.RegisterNativeCommand`) and runs through `Run(command line)`.
//! TODO: no engine subsystem defines one yet; call [`define`] then [`announce_all`] when one does.

use abi::callbacks::sandbox_con_var_system as managed;
use abi::slots::con_command as cc;
use abi::{NativeRef, Registry, StrRet, bind, class};
use std::sync::RwLock;

pub type Ref = NativeRef<class::ConCommand>;
/// Called with the arguments after the command name.
pub type Handler = Box<dyn Fn(&[&str]) + Send + Sync>;

pub struct ConCommand {
    pub name: String,
    pub help: String,
    pub flags: i64,
    run: Handler,
}

impl ConCommand {
    /// Run a full command line (`name arg arg`); quoted arguments keep their spaces.
    pub fn run(&self, line: &str) {
        let words = split(line);
        let args: Vec<&str> = words.iter().skip(1).map(String::as_str).collect();
        (self.run)(&args)
    }
}

fn split(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        let (word, next) = match rest.strip_prefix('"') {
            Some(q) => q.split_once('"').unwrap_or((q, "")),
            None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
        };
        words.push(word.to_owned());
        rest = next.trim_start();
    }
    words
}

static DEFINED: RwLock<Vec<Ref>> = RwLock::new(Vec::new());

/// Make an engine command; managed sees it after [`announce_all`].
pub fn define(name: &str, help: &str, flags: i64, run: Handler) -> Ref {
    let r = objects::create(ConCommand { name: name.into(), help: help.into(), flags, run });
    DEFINED.write().unwrap().push(r);
    r
}

/// Hand every defined command to managed's ConVarSystem. Calls managed: not from a `[nogc]` slot.
pub fn announce_all() {
    for &r in DEFINED.read().unwrap().iter() {
        managed::register_native_command(r);
    }
}

fn read<R: Default>(r: Ref, f: impl FnOnce(&ConCommand) -> R) -> R {
    objects::get::<_, ConCommand>(r).map(|c| f(&c)).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, cc::GET_NAME, |r| StrRet::from(read(r, |c| c.name.clone())));
    bind!(r, cc::GET_HELP_TEXT, |r| StrRet::from(read(r, |c| c.help.clone())));
    bind!(r, cc::GET_FLAGS, |r| read(r, |c| c.flags));
    bind!(r, cc::RUN, |r, line| read(r, |c| c.run(&line.map(|l| l.to_string_lossy()).unwrap_or_default())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn commands_run_with_their_arguments() {
        static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let r = define("echo_test", "says things", 0, Box::new(|a| *SEEN.lock().unwrap() = a.iter().map(|s| s.to_string()).collect()));
        read(r, |c| c.run(r#"echo_test hello "big world"  x"#));
        assert_eq!(*SEEN.lock().unwrap(), ["hello", "big world", "x"]);
        assert_eq!(read(r, |c| c.name.clone()), "echo_test");
    }
}
