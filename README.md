# stablehand

One home for agent engine accounts: how each logs in, where it may run, and
how much of its usage windows is left; and, from that, which account one
more session may start on now.

stablehand is being built: its commands land one ticket at a time, each
described here as it lands. The design is
[docs/adr/0001](docs/adr/0001-one-home-for-agent-accounts.md); the words are
in [GLOSSARY.md](GLOSSARY.md).

## The account register

```sh
stablehand account add claude-felix --engine claude-code --expires 2027-01-31   # a portable login
stablehand account add chatgpt --engine codex --machine mini --home /Users/agent/chatgpt   # a bound one
stablehand account list                     # every account, by name
stablehand account show chatgpt
stablehand account edit chatgpt --sessions 3 --unset home
stablehand account rm chatgpt
stablehand off claude-felix --reason "card declined"
stablehand on claude-felix
```

- An **account** has its engine (`claude-code`, `codex`, `pi`, `kimi`,
  `agy`, `grok`, `dsh`), its sessions at once (`--sessions`; a claude-code
  account without it runs 2, as Guild's ledger has it, the others no
  limit), and its login: **portable** without `--machine`, with when its
  token expires (`--expires YYYY-MM-DD`), or **bound** to the machine
  `--machine` names, in the home `--home` names.
- `account edit` changes only the terms given; `--unset machine|sessions|expires|home` clears one.
- `off` keeps who (`$USER`), when (UTC) and why; `on` forgets them.
- Each command prints one JSON object on stdout: `{"account": {…}}`, `{"accounts": […]}`, or for `rm` `{"name": …, "removed": true|false}`.
- A refusal prints `{"error": "…"}`, naming each fault with its fix, exits non-zero and writes nothing: an unknown engine, a name already kept, a date that is no calendar day, `--expires` on a bound login, `--home` on a portable one.
- `account rm` of an account not kept answers `"removed": false`, so running it twice leaves the same register.
- The register is one SQLite file, `$STABLEHAND_CONFIG_DIR/stablehand.db` (default `~/.config/stablehand`); the file is 0600, the directory 0700. It keeps no token: a portable login's token is sesame's.

## Logging in

```sh
stablehand account login claude-felix --expires 2027-11-24   # claude setup-token, then the token into sesame
stablehand account login kimi-1 --token < token.txt         # a token you have, pasted or piped
stablehand account login chatgpt                             # on mini: codex login, CODEX_HOME=/Users/agent/chatgpt
stablehand account show claude-felix   # {"account": {…}, "logged_in": true, "days_left": 365, "warning": null}
```

- A login is the engine's own (docs/adr/0001): stablehand runs the engine's
  command, never a vendor's OAuth client.
- **Portable**: `account login` runs the engine's token command (claude-code:
  `claude setup-token`; with `--token`, none), then sesame asks for the token
  with hidden input, or reads it from stdin when stdin is no terminal, and
  keeps it as the provider `stablehand-<name>`, made with `sesame provider add`
  the first time, its host the engine's usage host. The token is on no
  child's command line, in no log and not in the register; sesame gets it on stdin.
- **Bound**: on the account's machine (`$STABLEHAND_MACHINE`, else
  `hostname -s`), `account login` runs the engine's login with the variable
  it reads for its home set to `--home` (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
  `KIMI_CODE_HOME`, `GROK_HOME`, `HOME` for agy, `PI_CODING_AGENT_DIR`,
  `DSH_HOME`), and records when, as `logged_in_at`. Changing the account's
  engine, machine or home forgets it. It is refused on another machine.
- `account show` says whether the account is logged in: a bound one as
  recorded, a portable one when sesame holds its token. For a token with
  `--expires` it counts `days_left`, and from 30 days ahead its `warning`
  says to make a new one.
- A refused login exits 1 and writes nothing to the register: an engine with
  no token command without `--token`, `--token` or `--expires` on a bound
  login, a bound one with no `--home`, a token sesame refuses, an engine's
  login that fails.

## Importing from mytokens

```sh
stablehand account import --from-mytokens --dry-run   # what it would add, no token shown
stablehand account import --from-mytokens
```

- Each `agent-account-<name>` entry of this host's mytokens (`mytokens list`)
  becomes the account `<name>`: its engine from `HARNESS`, bound to `MACHINE`
  where the entry has one, else portable, with its `CLAUDE_CODE_OAUTH_TOKEN`
  kept in sesame as `account login` keeps one. The token goes from
  `mytokens get` to sesame's stdin, on no command line and in no answer.
- An account already kept is left as it is, answered `"already kept"`, so
  running it again adds nothing. mytokens is only read: nothing in it is changed or removed.
- `--dry-run` answers `"would add"` for each, reading no token and writing nothing.
- It refuses, importing nothing, an entry whose `HARNESS` is no engine, one
  with both a token and a `MACHINE`, and a field mytokens does not answer. A
  token sesame refuses stops it there: the accounts before it are added, and
  running it again adds the rest.

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
