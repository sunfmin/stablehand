# stablehand

One home for agent engine accounts: how each logs in, where it may run, and
how much of its usage windows is left; and, from that (`pick`), which account one
more session may start on now.

stablehand is being built: its commands land one ticket at a time, each
described here as it lands. The design is
[docs/adr/0001](docs/adr/0001-one-home-for-agent-accounts.md); the words are
in [GLOSSARY.md](GLOSSARY.md).

## The account register

```sh
stablehand account add claude-felix --engine claude-code --expires 2027-01-31 \
  --token-command 'mytokens get agent-account-claude-felix --field CLAUDE_CODE_OAUTH_TOKEN'   # a portable login
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
  limit), and its login: **portable** without `--machine`, with the command
  that prints its token (`--token-command`) and when the token expires
  (`--expires YYYY-MM-DD`), or **bound** to the machine `--machine` names, in
  the home `--home` names.
- `account edit` changes only the terms given; `--unset machine|sessions|expires|home|token-command` clears one.
- `off` keeps who (`$USER`), when (UTC) and why; `on` forgets them.
- Each command prints one JSON object on stdout: `{"account": {…}}`, `{"accounts": […]}`, or for `rm` `{"name": …, "removed": true|false}`.
- A refusal prints `{"error": "…"}`, naming each fault with its fix, exits non-zero and writes nothing: an unknown engine, a name already kept, a date that is no calendar day, `--expires` or `--token-command` on a bound login, `--home` on a portable one.
- `account rm` of an account not kept answers `"removed": false`, so running it twice leaves the same register.
- The register is one SQLite file, `$STABLEHAND_CONFIG_DIR/stablehand.db` (default `~/.config/stablehand`); the file is 0600, the directory 0700. It keeps no token: of a portable login it keeps the command that prints it.

## Logging in

```sh
CODEX_HOME=/Users/agent/chatgpt codex login     # on mini: the engine logs itself in
stablehand account edit claude-felix --token-command 'security find-generic-password -s claude-felix -w'
stablehand account show claude-felix   # {"account": {…}, "logged_in": true, "days_left": 365, "warning": null}
```

- stablehand runs no login and keeps no secret (docs/adr/0001). A person
  logs an engine in by the engine itself; stablehand finds the login where
  it is.
- **Bound**: logged in when its home, on its machine (`$STABLEHAND_MACHINE`,
  else `hostname -s`), holds its engine's login file (claude-code
  `.credentials.json`, codex and pi `auth.json`, kimi
  `credentials/kimi-code.json`, grok `auth.json`, dsh `.credentials.yaml`,
  agy `.gemini/antigravity-cli/antigravity-oauth-token` under its `HOME`),
  as of when that file was written: an engine logged in there before
  stablehand is adopted as it is, nothing written in the home. Its login
  line is the engine's own with the variable it reads for its home set to
  `--home` (`CLAUDE_CONFIG_DIR=<home> claude auth login`, `CODEX_HOME`,
  `KIMI_CODE_HOME`, `GROK_HOME`, `HOME` for agy, `PI_CODING_AGENT_DIR`,
  `DSH_HOME`).
- **Portable**: logged in when its `--token-command` prints its token: the
  first line of what `sh -c <cmd>` prints, run with no input, its stderr
  dropped, stopped after 20s (`$STABLEHAND_TOKEN_COMMAND_SECS`) with all it
  started. The token stays where the person keeps secrets: the Keychain
  (`security find-generic-password -s <service> -w`), 1Password (`op read
  op://<vault>/<item>/<field>`), mytokens (`mytokens get <entry> --field
  <field>`); claude-code's is made by `claude setup-token`. stablehand runs
  the command each time it needs the token and writes the token nowhere:
  it is on no child's command line, in no log and not in the register.
- A command that exits non-zero, prints no first line or runs past its
  bound is refused naming the command, never what it printed.
- Every refusal of an account not logged in names the line that logs it
  in: its engine's login line, or the `account edit --token-command` that
  names its token.
- `account show` says whether the account is logged in, `login_file`
  naming a bound one's file and when it was written (`{"path",
  "written_at"}`, null without one). For a token with `--expires` it counts
  `days_left`, and from 30 days ahead its `warning`
  says to make a new one.

## Importing from mytokens

```sh
stablehand account import --from-mytokens --dry-run   # what it would add, no token shown
stablehand account import --from-mytokens
```

- Each `agent-account-<name>` entry of this host's mytokens (`mytokens list`)
  becomes the account `<name>`: its engine from `HARNESS`, bound to `MACHINE`
  where the entry has one, else portable, its `--token-command` the
  `mytokens get <service> --account <label> --field CLAUDE_CODE_OAUTH_TOKEN`
  that prints its token: the token stays in mytokens, and the import reads
  none.
- A bound account is given the home factorium's workers use,
  `~/.agent-accounts/<name>` (`$HOME` of this host); nothing is made there.
- An account already kept is left as it is, answered `"already kept"`, so
  running it again adds nothing; but a kept bound account with no home is
  given that one, answered `"given its home"`, and a kept portable one with
  no token command its entry's, `"given its token command"`, so a run fills
  in what an earlier import left out. mytokens is only read: nothing in it is changed or removed.
- `--dry-run` answers `"would add"` (or `"would give its home"`, `"would give its token command"`) for each, writing nothing.
- It refuses, importing nothing, an entry whose `HARNESS` is no engine, one
  with both a token and a `MACHINE`, and a field mytokens does not answer.

## An account's credential

```sh
stablehand credential claude-felix          # export CLAUDE_CODE_OAUTH_TOKEN='sk-ant-…'
stablehand credential claude-felix --json   # {"name": "claude-felix", "login": "portable", "env": {"CLAUDE_CODE_OAUTH_TOKEN": "…"}}
stablehand credential chatgpt               # on mini: {"name": "chatgpt", "login": "bound", "files": [{"path": "/Users/agent/chatgpt/auth.json", "as": "auth.json"}]}
```

- stablehand answers the login; the caller makes the session. `credential`
  hands a session's caller an account's login and nothing else of it: the
  session's home is the caller's own (factorium builds a kept home per
  engine), so a person's settings, hooks or skills in the account's home
  never reach a worker. It only reads, and logs nothing in.
- **Portable**: the engine's token variable (claude-code:
  `CLAUDE_CODE_OAUTH_TOKEN`), its value the token its `--token-command`
  prints: one `export NAME='value'` line, the value one
  shell word whatever it holds, or with `--json` one object.
- **Bound**, on its machine only: the engine's login file in the account's
  home (`path`) and where the engine's home holds it (`as`), one JSON object
  with or without `--json`; the caller links `<its home>/<as>` to `path`, so a
  login the engine refreshes is refreshed in the account's home too. Each
  engine's login file is listed once, in stablehand (`stablehand credential
  --help`, "Engines"): the same file whose presence makes a bound home logged in.
- Nothing is written: the token goes to stdout alone, no file is left behind,
  and no child is given it on its command line.
- It refuses, exit 2 with `{"error": "…"}` naming the fix, an account that is
  off, not logged in (its token command refused as above, its home holding
  no login file), or bound to another machine; also one bound with no
  home, and a portable one of an engine with no token variable (any but
  claude-code: bind it). Exit 1 is an account not kept.
- factorium can hand a portable answer to a session unchanged (factorium SPEC
  §1.7; its test "an account secret stays off the command line"): its stdout
  is the very text factorium writes to a secrets file (`export NAME='value'`
  lines, `factorium_harness::secrets_file`). factorium writes that stdout as
  it is into a new 0600 file under its secrets directory, and the session's
  start line reads and removes it (`. <file> && rm -f <file> && …`); a
  non-zero exit is a session not started, its `error` the reason.

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
  account's bearer: a portable login's from its `--token-command`, a bound one's from `<home>/.credentials.json` (its
  `CLAUDE_CONFIG_DIR`; `~/.claude` without `--home`).
- The bearer goes to curl on its stdin, never on a command line.
- Its windows are `five_hour`, `seven_day` and any other the endpoint
  lists. A `claude setup-token` token is refused a usage query (HTTP 403
  scope): its reading spends one small turn instead, the question `claude -p
  --model haiku --output-format stream-json --verbose --tools ''
  --strict-mcp-config --no-session-persistence --setting-sources ''
  --system-prompt 'Answer with one word.' quota` (about 3s, under $0.0002),
  the token in its `CLAUDE_CODE_OAUTH_TOKEN`, run in a new empty config dir
  (`CLAUDE_CONFIG_DIR`) removed after, so nothing of `~/.claude` is read; its
  stream's `rate_limit_event` says the windows. Such an account is asked
  the question from then on, not its usage: at most once an hour, unless
  `--now`. Each read says `read_by` (`usage` or `question`) and
  `spent_turn`; `status` says the latest reading's `read_by`.
- agy's reader runs `agy -p /usage --output-format json` in the account's
  home (`HOME` set to `--home`; the user's own without it), on its machine
  only: no model turn, no conversation made. Its windows are each model
  group's buckets (`gemini-5h`, `gemini-weekly`, …), the group its `group`;
  a window reset in 1970 is one not yet used. A portable agy account is
  refused: bind it. A home not signed in fails after agy has waited 60s for
  a browser sign-in; an API-key login (exit 2) has no account quota.
- dsh's reader asks DeepSeek Platform
  (`platform.deepseek.com/api/v0/users/get_user_summary`) with the grant
  dsh keeps, `<home>/.credentials.yaml` (its `DSH_HOME`; `~/.dsh` without
  `--home`), or a portable login's from its `--token-command`, on curl's stdin with dsh's
  `x-client-*` headers. A DeepSeek account is pay-as-you-go: its reading is
  a **balance** per wallet (`normal`, `bonus`), no window, and `status`
  shows it under `balances`.
- A credential the engine refused (dsh: HTTP 401 or code 40003; dsh drops
  its grant then) is booked `"outcome": "refused"` and never asked again,
  `--now` or not, until it logs in again: its home's login file written
  after it, or a portable one given its `--token-command` again (`account
  edit`); `status` says `"refused": true`.
- codex's and pi's readers read a ChatGPT account's `primary` and
  `secondary` windows, where it answers them (used %, length, resets at), through the engine's own
  bound login, on its machine: codex's app server (`codex app-server`,
  `account/rateLimits/read`, in its `CODEX_HOME`; no turn spent), and for pi
  `chatgpt.com/backend-api/wham/usage` with the bearer `pi auth
  print-bearer-token --provider openai-codex` gives (in its
  `PI_CODING_AGENT_DIR`) and its login's `ChatGPT-Account-Id`, on curl's
  stdin. A codex and a pi login of one ChatGPT account (one
  `ChatGPT-Account-Id`) are one account: register it once.
- kimi's reader asks `api.kimi.com/coding/v1/usages`, as Kimi Code's own
  `/usage` does, with the account's bearer: a portable login's from its
  `--token-command`, a bound one's from
  `<home>/credentials/kimi-code.json` (its `KIMI_CODE_HOME`; `~/.kimi-code`
  without `--home`). Its windows are `limit_5h` and any other it lists
  (`limit_7d`, `limit_month_total`, `limit_month_code`). A bound login's
  token lasts 15 minutes and only a kimi session refreshes it: read long
  after one, it is refused (HTTP 401), said with that fix.
- grok's reader asks `cli-chat-proxy.grok.com/v1/billing?format=credits`
  with the bearer of the grok login in the account's home
  (`<home>/auth.json`, its `GROK_HOME`; `~/.grok` without `--home`), so a
  grok account is bound. Its token lasts hours: an expired one is refreshed
  by grok itself (`grok models`, no model turn). Its windows are the credit
  period's, `weekly`: the account's, and one per product in its `group`
  (`Api`, `GrokBuild`): a share of the account's credits, never paced on
  its own. A period just begun, its usage not in the answer, is one
  `weekly` window with nothing used.
- `limit` books a caller's report that a session met the account's limit: the window it names is used up until `--resets-at`, or until the next reading without it; without `--window`, the whole account is.
- `status` answers each account's windows (each with its `group` where the
  engine groups them), used %, resets at, source (`read` or `limit`),
  `stale`, and pace: `tight` (at its recent rate it runs out before it
  resets), `spare` (it resets within a quarter of its length with 20% or
  more left), `used_up`, or `steady`. The account's pace is its worst
  window's. The rules are Guild's ledger's: a reading over two hours old,
  or a window under half used, is never tight on its own.
- `account rm` removes the account's books with it.

## Claude Code's status line

```json
"statusLine": {"type": "command", "command": "stablehand statusline --account claude-felix --then ~/.claude/statusline.sh"}
```

- Set in `~/.claude/settings.json`, Claude Code runs it with its session's
  JSON on stdin; its `rate_limits` (`five_hour`, `seven_day`:
  `used_percentage`, `resets_at`) are booked as a reading of `--account`,
  as `read` books one, when they differ from the books' latest entry.
  Booking is local: it asks no engine and reaches no network (a few
  milliseconds a run).
- It prints one line for the bar, `claude-felix · 5h 24% · 7d 41%`. With
  `--then <cmd>`, `sh -c <cmd>` gets the same stdin byte for byte, and its
  line and exit are the answer, so a person's own status line keeps working.
- It never refuses: an account not kept, or not claude-code's, books
  nothing, and the line says why (with `--then`, stderr does).

## Picking an account

```sh
stablehand pick --engine claude-code --running claude-felix=1,claude-two=2
# {"account": "claude-felix"}
# {"none": true, "until": "2026-11-24T15:00:00Z", "why": [{"account", "reason", "why", "until"}, …]}
```

- `pick` answers which account one more session of the engine may start on
  now. It reads no engine: it judges the books as `status` shows them.
- The candidates are the engine's accounts usable on the machine
  (`--machine`, else this one): portable, or bound to it; logged in; not off.
- The rules are Guild's ledger's quota. None is picked on an account a
  limit holds on, until it lifts; any of whose windows is the stop percent
  used (`--stop-percent`, 90 unless it says), until that window resets;
  running its sessions at once; or one of whose windows, at its present
  rate shared by the sessions running and one more, runs out before it
  resets (a reading over two hours old, or a window under half used, runs
  on at no rate).
- Of the rest, the one with the most left of its most used window; one with
  no reading in force, or only stale ones, after every one with one; of
  equals, the first by name.
- `--running <account>=<n>,…` is the caller's count of the sessions now
  running on each account (factorium knows its workers); stablehand counts
  none itself, and counts across machines only as the caller does. A name in
  it that is no account kept is refused.
- None answers `until`, the earliest any passed over lifts (null when none
  says when: off, not logged in, full), and each one's `reason`: `off`,
  `elsewhere`, `not_logged_in`, `limit`, `stopped`, `full` or `runs_out`.

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

### How help is written

`stablehand --help` (the index), `stablehand <command> --help`,
`stablehand help <topic>` and `stablehand help all` (the whole manual) are
one manual, assembled by [agent-help](https://github.com/sunfmin/agent-cli)
from Markdown doc comments on the thing each fact belongs to: the root and
each command's on clap's types in `src/main.rs`; the refusal codes on the
`Code` enum in `src/refusal.rs`, each variant's doc its line in the manual
and its name the `code` a refusal prints; the engines' logins generated
from the one list of engines (`Engine`). No help is written anywhere else,
and no `long_about` or `after_help` by hand. The test `the_manual_is_whole`
runs `agent_help::check` and clap's `debug_assert` on it.
