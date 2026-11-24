//! The readers, one per engine (GLOSSARY.md "Reader"): each asks its
//! engine for an account's reading and turns the answer into what the books
//! keep, windows or, of a pay-as-you-go engine, balances; the only code
//! that knows the engine's endpoint or command. A new engine's reader is a
//! file here and a row in [`READERS`]; one that answers windows or why not
//! is adapted by [`windows`].
use crate::account::{Account, Engine};
use crate::books::{Status, What, Window};

mod agy;
mod chatgpt;
pub mod claude_code;
mod dsh;
mod grok;
mod kimi;

/// A reader: what to book of an account now, given what its books say
/// ([`Status`]), a reading ([`What::Read`], [`What::Balance`]) or a try that
/// failed ([`What::Failed`], [`What::Refused`]). It never puts a token in
/// what it answers.
pub type Read = fn(&Account, &Status) -> What;

/// Each engine that has a reader, and its reader.
const READERS: &[(Engine, Read)] = &[
    (Engine::ClaudeCode, |a, s| {
        windows(claude_code::read(a, s.read_by))
    }),
    (Engine::Codex, |a, _| windows(chatgpt::codex(a))),
    (Engine::Pi, |a, _| windows(chatgpt::pi(a))),
    (Engine::Kimi, |a, _| windows(kimi::read(a))),
    (Engine::Grok, |a, _| windows(grok::read(a))),
    (Engine::Agy, |a, _| windows(agy::read(a))),
    (Engine::Dsh, |a, _| dsh::read(a)),
];

/// A reader's windows, or why its engine gave none, as the books keep them.
fn windows(read: Result<Vec<Window>, String>) -> What {
    read.map_or_else(What::Failed, What::Read)
}

/// The reader of `engine`, where it has one.
pub fn of(engine: Engine) -> Option<Read> {
    READERS.iter().find(|(e, _)| *e == engine).map(|(_, r)| *r)
}
