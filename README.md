# stablehand

One home for agent engine accounts: how each logs in, where it may run, and
how much of its usage windows is left; and, from that, which account one
more session may start on now.

stablehand is being built: today it says only its version
(`stablehand --version`). Its commands land one ticket at a time, each
described here as it lands. The design is
[docs/adr/0001](docs/adr/0001-one-home-for-agent-accounts.md); the words are
in [GLOSSARY.md](GLOSSARY.md).

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
issue (`docs/agents/issue-tracker.md`).

```sh
./gate.sh   # fmt, clippy, slimfit check, the release dry run, truefit contract
```
