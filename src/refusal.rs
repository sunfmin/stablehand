//! Why a command refused: its code, the one home of what a caller can tell
//! apart, and the text that names each fault with its fix.

/// Why stablehand refused, as a refusal's `code`. Each refusal's `error` names every fault with the line that fixes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, agent_help::Help)]
pub enum Code {
    /// The command line is not one stablehand reads, an unknown engine among it, or `help` was asked for no topic it has. Exit 2.
    Usage,
    /// No account by that name: `stablehand account list` names those kept.
    NoAccount,
    /// `account add` of a name already kept: change it with `account edit`, or `account rm` it first.
    NameTaken,
    /// An account's terms as given are not ones it can have: a date that is no day, `--expires` or `--token-command` on a bound login, `--home` on a portable one, an empty `--reason` or `--token-command`.
    BadTerms,
    /// `credential` of an account no session may start on here: off, not logged in (its token command answers no token, its home holds no login file), with no home, bound to another machine, or of an engine with no token variable. Exit 2.
    NoSession,
    /// `limit` given a `--resets-at` that is no RFC 3339 time or is past, or an empty `--window`.
    BadLimit,
    /// `account scan --path` names no directory.
    BadPath,
    /// Something stablehand reads or runs failed (the register, mytokens, an engine's reader): the text says what, and how to fix it.
    Fault,
}

impl Code {
    /// The exit status a refusal under this code ends with.
    pub fn exit(self) -> u8 {
        match self {
            Code::Usage | Code::NoSession => 2,
            _ => 1,
        }
    }
}

/// A refusal: its code, and the text naming each fault with its fix.
#[derive(Debug, PartialEq, Eq)]
pub struct Refused {
    pub code: Code,
    pub text: String,
}

impl Refused {
    pub fn new(code: Code, text: impl Into<String>) -> Self {
        Refused {
            code,
            text: text.into(),
        }
    }
}

/// A failure of something stablehand reads or runs, said as text.
impl From<String> for Refused {
    fn from(text: String) -> Self {
        Refused::new(Code::Fault, text)
    }
}
