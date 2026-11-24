//! The readers, one per engine (GLOSSARY.md "Reader"): each asks its
//! engine for an account's reading and turns the answer into windows, or,
//! of a pay-as-you-go engine, balances; the only code that knows the
//! engine's endpoint or command. A new engine's reader is a file here and a
//! row in [`READERS`] (windows) or [`BALANCE_READERS`] (balances).
use crate::account::{Account, Engine};
use crate::books::{Balance, What, Window};

mod claude_code;
mod dsh;

/// A reader: an account's windows now, or why its engine gave none. It
/// never puts a token in what it answers.
pub type Read = fn(&Account) -> Result<Vec<Window>, String>;

/// A balance reader: a pay-as-you-go account's wallets now, or why its
/// engine gave none. It never puts a token in what it answers.
pub type ReadBalances = fn(&Account) -> Result<Vec<Balance>, Fault>;

/// Why a balance reader's engine gave no reading.
#[derive(Debug, PartialEq)]
pub enum Fault {
    /// The ask failed (unreachable, an error, an answer it cannot read).
    Failed(String),
    /// The engine refused the credential: asking again with it is no use,
    /// and the engine itself drops it (dsh on a 401 or code 40003).
    Refused(String),
}

/// Each engine that has a reader of windows, and its reader.
const READERS: &[(Engine, Read)] = &[(Engine::ClaudeCode, claude_code::read)];

/// Each engine that has a reader of balances, and its reader.
const BALANCE_READERS: &[(Engine, ReadBalances)] = &[(Engine::Dsh, dsh::read)];

/// An engine's reader, of either kind.
#[derive(Clone, Copy)]
pub enum Reader {
    Windows(Read),
    Balances(ReadBalances),
}

impl Reader {
    /// Reads `account`: what to book of it, a reading or a try that failed.
    pub fn read(self, account: &Account) -> What {
        match self {
            Reader::Windows(read) => read(account).map_or_else(What::Failed, What::Read),
            Reader::Balances(read) => match read(account) {
                Ok(balances) => What::Balance(balances),
                Err(Fault::Failed(why)) => What::Failed(why),
                Err(Fault::Refused(why)) => What::Refused(why),
            },
        }
    }
}

/// The reader of `engine`, where it has one.
pub fn of(engine: Engine) -> Option<Reader> {
    let windows = READERS.iter().find(|(e, _)| *e == engine);
    let balances = BALANCE_READERS.iter().find(|(e, _)| *e == engine);
    windows
        .map(|(_, r)| Reader::Windows(*r))
        .or_else(|| balances.map(|(_, r)| Reader::Balances(*r)))
}
