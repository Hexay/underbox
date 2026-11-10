//! Where the ABI layer's diagnostics go. The engine installs a sink that forwards to managed's log;
//! until then they go to stderr.

use std::sync::OnceLock;

pub type Sink = fn(Level, &str);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

static SINK: OnceLock<Sink> = OnceLock::new();

pub fn set_sink(sink: Sink) {
    let _ = SINK.set(sink);
}

pub fn log(level: Level, message: &str) {
    match SINK.get() {
        Some(sink) => sink(level, message),
        None => eprintln!("[{level:?}] {message}"),
    }
}

pub fn warn(message: &str) {
    log(Level::Warn, message)
}
