//! stablehand: one home for agent engine accounts, their logins and their
//! usage windows (README.md, docs/adr/0001). Commands arrive one ticket at a
//! time; until then it says its version and what it is.

// build.rs runs it to stamp the version; compiled here too, for its tests.
#[cfg_attr(not(test), allow(dead_code))]
mod version;

use clap::Parser;

const FOR_AI: &str = "\
stablehand keeps each agent engine account (Claude Code, Codex, pi, Kimi Code,
agy, Grok, dsh): how it logs in, where it may run, and how much of its usage
windows is left. No command is implemented yet: see README.md.";

#[derive(Parser)]
#[command(name = "stablehand", version = env!("STABLEHAND_VERSION"), before_help = FOR_AI)]
struct Cli {}

fn main() {
    Cli::parse();
}
