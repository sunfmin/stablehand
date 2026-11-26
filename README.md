# stablehand

One home for agent engine accounts: how each logs in, where it may run, and
how much of its usage windows is left; and from that, which engine, model
and account the next session starts on. It knows Claude Code
(`claude-code`), Codex (`codex`), pi, Kimi Code (`kimi`), agy, Grok (`grok`)
and dsh.

## What you need

stablehand installed (see Install) and at least one engine logged in by its
own login command (`claude auth login`, `codex login`, …). stablehand runs no
login and keeps no secret: it finds the logins where the engines keep them.

A **portable** login, a token rather than a home (`claude setup-token` makes
one), is added with the command that prints the token. Any command that
prints it will do: the Keychain's `security find-generic-password -s <service> -w`,
1Password's `op read …`, or mytokens, if you use it.

```sh
stablehand account add claude-work --engine claude-code --expires 2027-01-31 \
  --token-command 'security find-generic-password -s claude-work -w'
```

## A first run

```sh
stablehand account scan                     # the logins on this machine, each with the name it would get
stablehand account scan --add               # add them to the register (claude-code, codex, …)
stablehand read                             # ask each account's engine for its usage windows
stablehand status                           # what the books say now: each window, its pace
stablehand order set claude-code@opus codex # the order to pick in: engines, and their models
stablehand pick                             # what the next session starts on, and why not the others
stablehand run claude -p hi                 # start claude on its pick, login handed in; or say why none
```

Each of these but `run` answers one JSON object on stdout; a refusal is
`{"error", "code"}` with the line that fixes it.

## Where it keeps things

The register and the books are one SQLite file,
`$STABLEHAND_CONFIG_DIR/stablehand.db` (default `~/.config/stablehand`). It
holds no token: of a portable login it keeps only the command that prints
it, run each time the token is needed.

## The manual

Every command's details are in the manual: `stablehand --help` (the index),
`stablehand <command> --help`, `stablehand help <topic>`, and
`stablehand help all` (the whole of it). The design is in
[docs/adr/](docs/adr/); the words are in [GLOSSARY.md](GLOSSARY.md).

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
each command's on clap's types in `src/main.rs`, the refusal codes on the
`Code` enum in `src/refusal.rs`, the engines' logins from the one list of
engines (`Engine`). No help is written anywhere else, and no `long_about` or
`after_help` by hand. The test `the_manual_is_whole` checks it.
