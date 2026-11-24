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

## The books: read, limit, status

```sh
stablehand read claude-felix        # ask its engine, book the reading
stablehand read                     # every account
stablehand read claude-felix --now  # even if read in the last hour
stablehand limit claude-felix --window five_hour --resets-at 2026-11-24T15:00:00Z
stablehand status                   # each account's windows and their pace
```

- `read` asks the account's engine through its **reader** and books what it
  answered. An account read or tried in the last hour waits (`"outcome":
  "waits"`, with `next_at`) unless `--now`.
- A reader's failure is booked as a failed try and said (`"outcome": "failed"`, with its `"error"` and fix), and still exits 0; the account's last windows are kept, marked `"stale": true`.
- An engine with no reader yet answers `"outcome": "no_reader"` and books nothing.
- claude-code's reader asks `api.anthropic.com/api/oauth/usage` with the
  account's bearer: a portable login's from sesame (`sesame token <name>`),
  a bound one's from `<home>/.claude/.credentials.json`.
- The bearer goes to curl on its stdin, never on a command line.
- Its windows are `five_hour`, `seven_day` and any other the endpoint
  lists. A `claude setup-token` token is refused a usage query (HTTP 403):
  such an account is known by `limit` alone.
- `limit` books a caller's report that a session met the account's limit: the window it names is used up until `--resets-at`, or until the next reading without it; without `--window`, the whole account is.
- `status` answers each account's windows (each with its `group` where the
  engine groups them), used %, resets at, source (`read` or `limit`),
  `stale`, and pace: `tight` (at its recent rate it runs out before it
  resets), `spare` (it resets within a quarter of its length with 20% or
  more left), `used_up`, or `steady`. The account's pace is its worst
  window's. The rules are Guild's ledger's: a reading over two hours old,
  or a window under half used, is never tight on its own.
- `account rm` removes the account's books with it.

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
