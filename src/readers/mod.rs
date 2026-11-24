//! The readers, one per engine (GLOSSARY.md "Reader"): each asks its
//! engine for an account's reading and turns the answer into windows, the
//! only code that knows the engine's endpoint or command. A new engine's
//! reader is a file here and a row in [`READERS`].
use crate::account::{Account, Engine};
use crate::books::Window;

mod chatgpt;
mod claude_code;

/// A reader: an account's windows now, or why its engine gave none. It
/// never puts a token in what it answers.
pub type Read = fn(&Account) -> Result<Vec<Window>, String>;

/// Each engine that has a reader, and its reader.
const READERS: &[(Engine, Read)] = &[
    (Engine::ClaudeCode, claude_code::read),
    (Engine::Codex, chatgpt::codex),
    (Engine::Pi, chatgpt::pi),
];

/// The reader of `engine`, where it has one.
pub fn of(engine: Engine) -> Option<Read> {
    READERS.iter().find(|(e, _)| *e == engine).map(|(_, r)| *r)
}
