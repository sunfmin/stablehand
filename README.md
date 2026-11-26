# stablehand

One home for agent engine accounts: how each logs in, where it may run, and
how much of its usage windows is left; and, from that and the order of
engines and models kept beside them, what one more session may start on
now, and how this host starts each engine. The rest of the session is the
caller's.

The design is
[docs/adr/0001](docs/adr/0001-one-home-for-agent-accounts.md), each later
decision an ADR in `docs/adr/`; the words are in [GLOSSARY.md](GLOSSARY.md).

## What it keeps where

- **The manual** is the binary's help: `stablehand --help` lists the
  commands, `stablehand <command> --help` says all one does, answers and
  refuses, and `stablehand help all` is the whole of it. This README
  describes no command.
- **The register**, books, order and engines' commands are one SQLite file
  (`stablehand --help` says where). It keeps no
  token: stablehand runs no login and finds each login where its engine
  keeps it.

```sh
stablehand account scan --add   # register the logins on this machine
stablehand status               # each account's windows and their pace
stablehand run claude -p hi     # a session on the account stablehand picks
```

## Install

```sh
brew install sunfmin/tap/stablehand
```

factorium cuts and publishes each version (`factorium.toml`'s `[cut]`,
`packaging/release.sh`); sunfmin/homebrew-tap's release workflow signs and
notarizes it.

## Working on stablehand

A Rust command-line program, edition 2024, the toolchain pinned in
`rust-toolchain.toml`. The repo is a factorium checkout: work is a factorium
issue.

```sh
./gate.sh   # fmt, clippy, slimfit check, the release dry run, truefit contract
```

### How help is written

The manual is assembled by [agent-help](https://github.com/sunfmin/agent-cli)
from Markdown doc comments on the thing each fact belongs to: the root and
each command's on clap's types in `src/main.rs`; the refusal codes on the
`Code` enum in `src/refusal.rs`; the engines' logins from the one list of
engines. No help is written anywhere else, and no `long_about` or
`after_help` by hand; a test runs `agent_help::check` on it.

A fact a change adds goes where it is true: a command's behaviour in its
help, a decision and its why in an ADR, a word in the glossary. This README
changes only when what stablehand is, or how it is built, does.
