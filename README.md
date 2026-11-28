# stablehand

One home for agent engine accounts: how each logs in, where it may run, and
how much of its usage windows is left; and from that, which engine, model
and account the next session starts on.

## A first run

Log at least one engine in by its own login command first: stablehand runs
no login and keeps no secret, it finds the logins where the engines keep them.

```sh
stablehand account scan --add               # add the logins on this machine to the register
stablehand read                             # ask each account's engine for its usage windows
stablehand order set claude-code@opus codex # the order to pick in: engines, and their models
stablehand run claude -p hi                 # start claude on its pick, login handed in
```

## The manual

The engines it knows, a portable login (`stablehand account add --help`),
where it keeps things and every command are in the manual: `stablehand --help`
and `stablehand help all`. The design is in [docs/adr/](docs/adr/); the words
are in [GLOSSARY.md](GLOSSARY.md).

## Install

```sh
brew install sunfmin/tap/stablehand
```

## Working on stablehand

See [CLAUDE.md](CLAUDE.md): the repo is a factorium checkout, and `./gate.sh`
must pass before anything lands.
