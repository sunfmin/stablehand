# stablehand

One home for agent engine accounts: how each logs in, where it may run, and
how much of its usage windows is left; and, from that and the order of
engines and models kept beside them (`pick`), what one more session may
start on now: its engine, model and account. The session itself is the
caller's.

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
stablehand account edit chatgpt --unset home
stablehand account rm chatgpt
stablehand off claude-felix --reason "card declined"
stablehand on claude-felix
```

- An **account** has its engine (`claude-code`, `codex`, `pi`, `kimi`,
  `agy`, `grok`, `dsh`) and its login: **portable** without `--machine`, with the command
  that prints its token (`--token-command`) and when the token expires
  (`--expires YYYY-MM-DD`), or **bound** to the machine `--machine` names, in
  the home `--home` names.
- `account edit` changes only the terms given; `--unset machine|expires|home|token-command` clears one.
- `off` keeps who (`$USER`), when (UTC) and why; `on` forgets them.
- Each command prints one JSON object on stdout: `{"account": {…}}`, `{"accounts": […]}`, or for `rm` `{"name": …, "removed": true|false}`.
- A refusal prints `{"error": "…"}`, naming each fault with its fix, exits non-zero and writes nothing: an unknown engine, a name already kept, a date that is no calendar day, `--expires` or `--token-command` on a bound login, `--home` on a portable one.
- `account rm` of an account not kept answers `"removed": false`, so running it twice leaves the same register.
- The register is one SQLite file, `$STABLEHAND_CONFIG_DIR/stablehand.db` (default `~/.config/stablehand`); the file is 0600, the directory 0700. It keeps no token: of a portable login it keeps the command that prints it. The default is a released stablehand's only: a development build (`cargo build`, any version with `+`) refuses unless `STABLEHAND_CONFIG_DIR` is set, as its schema steps may not be the released ones. A store migrated by other steps than this stablehand's is refused, naming the step.

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

## Finding the logins on this machine

```sh
stablehand account scan                  # what it finds, writing nothing
stablehand account scan --add            # register the new ones
stablehand account scan --path /Users/agent/chatgpt   # one more home to look in
```

- scan looks in each engine's own default home (`~/.claude`, `~/.codex`,
  `~/.pi/agent`, `~/.kimi-code`, agy's `$HOME`, `~/.grok`, `~/.dsh`) and
  in each `--path`, never in a home another program keeps for its own use
  (factorium's `~/.agent-accounts`, Guild's `~/.guild/engines`) unless a
  `--path` names it. A home is a login when it holds its engine's login
  file.
- It reads mytokens too, when it is on PATH, from `mytokens list` alone,
  which names each entry's fields and never prints a value: an
  `agent-account-<name>` entry with an engine's token field
  (`CLAUDE_CODE_OAUTH_TOKEN`) is a portable account `<name>`, its
  `--token-command` `mytokens get agent-account-<name> --field <that
  field>`; one with no token (a bound one, its `MACHINE`) is `skipped`. No
  token is read.
- **One account per subscription.** The same subscription is often in
  several homes (pi's `openai-codex` and codex on one ChatGPT account).
  scan tells them apart by the identity each login file holds (Claude's
  account and organization, ChatGPT's account id, kimi's user id, agy's
  id token's email, grok's user id), read and compared in memory, never
  printed, logged or stored: it answers, and keeps on an account it adds,
  the identity's hash. Of one identity's logins the account is the one an
  account kept already is, else the most recently written; the others are
  `copy`, with the warning that copies of one refresh token refresh each
  other out. An identity it cannot read (a format it does not know; dsh's
  file holds none) is said as such, `identity_unknown`, never guessed. A
  portable token tells no identity, so whether it is a home login's
  subscription is `unknown`.
- Without `--add` it writes nothing: it answers each login found (engine,
  home, logged in at, identity's hash, `new`, `registered` or `copy`, and
  the account: the one it is, or is a copy of, or the name `--add` gives,
  `<engine>`, or `<engine>-2` … when the name is kept). An account kept is
  never renamed.
- With `--add` it registers the `new` ones and nothing else: a login as a
  bound account on this machine in its home (`account add --machine <this>
  --home <dir>`), a token as a portable account with its token command.
  Run again, it adds nothing.

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
  a window reset in 1970 is one not yet used. Each window's `models` are
  its group's, by the ids `agy models` (run after it, in the same home)
  gives: a model named in the group's description, with any variant
  (`Claude Opus 5.5` holds `claude-opus-5-5-high`). A portable agy account is
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
  engine groups them, and the `models` whose use it counts where the engine
  names them), used %, resets at, source (`read` or `limit`),
  `stale`, and pace: `tight` (at its recent rate it runs out before it
  resets), `spare` (it resets within a quarter of its length with 20% or
  more left), `used_up`, or `steady`. The account's pace is its worst
  window's. The rules are Guild's ledger's: a reading over two hours old,
  or a window under half used, is never tight on its own.
- `account rm` removes the account's books with it.

## Claude Code's status line

```json
"statusLine": {"type": "command", "command": "stablehand statusline --then 'bash \"$HOME/.claude/statusline.sh\"'"}
```

- Set in `~/.claude/settings.json`, Claude Code runs it with its session's
  JSON on stdin; its `rate_limits` (`five_hour`, `seven_day`:
  `used_percentage`, `resets_at`) are booked as a reading of the session's
  account, as `read` books one, when they differ from the books' latest
  entry. With no account, nothing is booked and stablehand's line is empty
  (with `--then`, the person's line alone).
- The account is `--account`, else `$STABLEHAND_ACCOUNT` (`stablehand run`
  sets it in the session it starts), else found from the session's
  environment, no token command run: none with `ANTHROPIC_BASE_URL` or
  `CLAUDE_CODE_OAUTH_TOKEN` set; otherwise the claude-code account bound to
  this machine whose home is the session's config dir (`$CLAUDE_CONFIG_DIR`,
  else `~/.claude`; resolved, symlinks followed), unless it is off.
  `--then`'s command gets it in `$STABLEHAND_ACCOUNT` (unset with none), to
  show it or read its books (`stablehand status "$STABLEHAND_ACCOUNT"`).
  Booking is local: it asks no engine and reaches no network (a few
  milliseconds a run).
- Where stdin carries no `rate_limits` and the account was not read or
  tried in the last ten minutes, it starts `stablehand read --now
  <account>` detached and answers at once from the books as they are; the
  next refresh shows the new reading. One such read of an account runs at
  a time (`reading-<account>.lock` beside the register, let go when it
  ends). A `claude setup-token` account's read spends one smallest-model
  turn: at most about 6 an hour.
- It prints one line for the bar, `claude-felix · 5h 24% · 7d 41%`. Where
  stdin carries no `rate_limits` (a `claude setup-token` session's never
  does), the windows are the books' latest, as `status` has them, each
  with its age past ten minutes (`7d 70% (3h ago)`). With
  `--then <cmd>`, `sh -c <cmd>` gets the same stdin byte for byte, and its
  line and exit are the answer, so a person's own status line keeps working.
- It never refuses: an account not kept, or not claude-code's, books
  nothing, and the line says why (with `--then`, stderr does).

## The order

```sh
stablehand order set claude-code@opus codex grok agy@gemini-4-neon-high,claude-opus-5-5-high
stablehand order show   # {"order": [{"engine": "claude-code", "models": ["opus"]}, …]}
```

- The engines, and of each the models, the next session starts on,
  highest first; an engine with no model runs its own. One order per
  machine, kept in the register's database; `set` replaces it.
- An engine is one stablehand knows; agy's models are checked against the
  ids `agy models` lists, run in the home of the first agy account bound
  to this machine (the user's own without one). Other engines' models are
  kept as given. A fault is refused (`bad_order`), writing nothing.
- It is a preference only: the command an engine starts with, its flags,
  its guard, and which checkout keeps to which engine stay the caller's
  (factorium's kit commands and pins).

## Picking what the next session starts on

```sh
stablehand pick --running claude-felix=1,claude-two=2
stablehand pick --engine agy --model claude-opus-5-5-high
# {"engine": "agy", "model": "claude-opus-5-5-high", "account": "agy", "why": […], "model_in_no_group": []}
# {"none": true, "until": "2026-11-24T15:00:00Z", "why": [{"engine", "model", "account", "reason", "why", "until"}, …], "model_in_no_group": []}
```

- `pick` walks the order: each engine, and of each its models in turn;
  at each (engine, model) the engine's accounts, judged as below. It
  answers the first that takes one more session, with `why` each one
  before it was passed over; or none, until the earliest any lifts.
- An engine of the order with no account kept is skipped and said
  (`no_account`): nothing starts on a login stablehand does not keep.
- `--engine` (repeatable) and `--model` narrow the walk to them, in the
  order's order; one not in the order may still be named and is tried
  after the rest. With no order and no `--engine`, nothing is walked:
  refused (`no_order`).
- It reads no engine: it judges the books as `status` shows them.
- The candidates are the engine's accounts usable on the machine
  (`--machine`, else this one): portable, or bound to it; logged in; not off.
- The rules are Guild's ledger's quota. None is picked on an account a
  limit holds on, until it lifts; any of whose windows is the stop percent
  used (`--stop-percent`, 90 unless it says), until that window resets;
  or one of whose windows, at its present
  rate shared by the sessions running and one more, runs out before it
  resets (a reading over two hours old, or a window under half used, runs
  on at no rate). How many sessions run on an account never passes it over
  by itself: they count only in that rate.
- Of the rest, the one with the most left of its most used window; one with
  no reading in force, or only stale ones, after every one with one; of
  equals, the first by name.
- `--running <account>=<n>,…` is the caller's count of the sessions now
  running on each account (factorium knows its workers), counted beside the
  sessions `stablehand run` keeps; across machines only as the caller counts. A name in
  it that is no account kept is refused.
- At a step with a model (agy's `claude-opus-5-5-high`, as `agy models`
  lists it), an account whose windows are grouped by model is judged only by the
  windows of the group holding it, and those in no group, so one agy
  account is two pools: its Gemini group used up, the walk falls to its
  Claude model. One no group of which holds the model is judged by
  every window, and named in the answer's `model_in_no_group`; an engine
  that groups no window is judged as without a model.
- None answers `until`, the earliest any passed over lifts (null when none
  says when: off, not logged in, no account), and each one's
  `reason`: `no_account`, `off`, `elsewhere`, `not_logged_in`, `limit`,
  `stopped` or `runs_out`.

## Starting a session on the pick

```sh
stablehand run claude -p hi          # claude-code, on the account (and model) pick answers
stablehand run                       # the order's pick, its engine's own program, interactive
stablehand run --account claude-felix --model opus claude
stablehand run --home /tmp/s1 codex  # a session home of its own, the login linked in
# stderr: stablehand: claude-code on claude-felix (opus)
```

- The engine is the command's program (`claude` claude-code, `codex`,
  `pi`, `kimi`, `agy`, `grok`, `dsh`), else `--engine`, else
  `--account`'s. With no command the whole order is walked and the picked
  engine's own program starts (dsh's `dsh tui`). Arguments with no command
  (`stablehand run -- -p hi`) are refused: they mean something else to each
  engine.
- The engine, model and account are `pick`'s; `--engine`, `--account`,
  `--model`, and a model the command names, narrow it. None picked starts
  nothing and exits 1, `{"error", "code": "none_picked", "until", "why"}`.
- The login reaches the child in its environment alone: portable, its
  token variable (`CLAUDE_CODE_OAUTH_TOKEN`) from the account's token
  command; bound, its home variable (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
  `HOME` for agy, …) set to the account's home. With `--home <dir>` the
  home variable is `<dir>` and a bound login's files are linked into it;
  refused for a bound claude-code account, since Claude Code replaces a
  linked login with a copy when it refreshes (docs/adr/0002).
- The child's environment names its account, `STABLEHAND_ACCOUNT=<account>`
  (a name, never a value), so its status line shows that account's windows.
- Where the command names no model, the engine's model flag and the picked
  model are added (`claude --model opus -p hi`); dsh takes none.
- stderr gets one line of what was chosen, never a value. run waits for the
  child: the terminal is its, its exit code is stablehand's, a signal that
  ends it ends stablehand alike, and a signal sent to stablehand is passed on.
- While it runs, its session is kept in the register and `pick` counts it
  beside `--running`; it is dropped when it ends, or found dead by its pid.

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
