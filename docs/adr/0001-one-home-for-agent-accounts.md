# 0001. One home for agent accounts: logins and usage together, readers per engine

Status: accepted, 2026-11-24 (Felix, control session); the command an engine starts with is stablehand's since 0003

## Context

An agent account's facts had four homes: mytokens' `agent-account-*`
entries, the account code of factorium's harness, Guild's ledger crate, and
afkconvoy's window readers. Each read a different part, and the harnesses
said grok, pi and dsh "report none". On 2026-11-24 every engine Felix uses
was asked for its usage on this Mac, live, with no model turn:

| Engine | How it is asked | What it says |
|---|---|---|
| claude-code | `GET api.anthropic.com/api/oauth/usage`, the OAuth bearer and `anthropic-beta: oauth-2025-04-20`; also the status line's stdin `rate_limits` | `five_hour`, `seven_day`: used %, resets at |
| codex | its app server | `primary`, `secondary` |
| pi | its `openai-codex` login: `GET chatgpt.com/backend-api/wham/usage`, bearer and `ChatGPT-Account-Id` | `rate_limit.primary_window`: used %, reset at |
| kimi | `/usages` | `limit_5h` |
| agy | `agy -p "/usage" --output-format json` (0 turns) | per model group, `5h` and `weekly`: remaining fraction, reset time (1970 = unused) |
| grok | `GET cli-chat-proxy.grok.com/v1/billing?format=credits`, bearer from `~/.grok/auth.json` | weekly period, `creditUsagePercent`, per product |
| dsh | `GET platform.deepseek.com/api/v0/users/get_user_summary`, `x-dsh-auth-token` | wallets' balance; no window |

None of these endpoints is documented for outside use.

## Decision

- **stablehand is the one home** of an account: its engine, its login, where
  it may run, its terms (a token's expiry), its readings
  and its off switch. mytokens' `agent-account-*` entries move here and go;
  factorium asks stablehand for a pick and an account's credential.
- **An account is a subscription, not an engine.** Engines that spend one
  account (pi and codex on one ChatGPT account) read and book into the same
  account.
- **stablehand runs no login and keeps no secret** (Felix, 2026-11-24). The register is stablehand's: an account's engine, where it
  runs, its terms. A secret stays where the person keeps it, and stablehand
  keeps the command that reads it. A person logs an engine in by the engine
  itself (`codex login`, `claude setup-token`, …); stablehand never runs a
  login or a vendor's OAuth client. A bound account is logged in when its
  home holds its engine's login file, as of when that was written; a
  portable one when the command it names (`--token-command`: the Keychain's
  `security`, 1Password's `op read`, `mytokens get`) prints its token, run
  each time it is needed, bounded in time, the token written nowhere. What
  each engine's login is stays in stablehand as help: a refusal of an
  account not logged in names the line that logs it in.
  Before, a portable token was kept in sesame (`stablehand-<name>`); its
  only portable accounts were claude-code `setup-token` tokens, which
  neither refresh nor read usage, so sesame was only a second copy of what
  mytokens already held, and is gone.
- **One reader per engine**, the only code that knows its endpoint or
  command, each held to the real engine by a truefit contract. A reader that
  fails leaves the account's windows as last read, stale, and a caller's
  `limit` stands in until it reads again.
- **Reading is bounded**: at most once an hour per account unless a caller
  asks, never again on a credential the engine refused (dsh deletes its grant
  on a 401) until a login after it: a bound home's login file written since,
  or a portable account given its token command again.
- **stablehand the login, the caller the session.** `stablehand credential`
  answers an account's login and nothing else of it: a portable token, or a
  bound account's engine login files with where the engine's home holds them.
  The session's home is the caller's own, which links only those files into
  it, so nothing else of the account's home (a person's settings, hooks,
  skills) reaches a session. Each engine's login file is listed once, here:
  what a caller links is what makes a bound home logged in.
- **The pick is stablehand's**: the stop percent, the pace and the sessions
  already running decide it here, so no caller computes it again.
- **So is the order, and the whole pick** (Felix, control
  session, 2026-11-24). stablehand answers the accounts, their usage, and
  what the next session starts on: which engine, which model and which
  account, the first of the **order** (engines and their models, highest
  first, one per machine, in the register's database) whose account takes
  one more session. The order was factorium's kit and the try-the-next
  loop would have been factorium's; here, every caller (factorium, a
  person, Guild) gets one answer. The session itself stays the caller's:
  the command an engine starts with, its flags, its guard, and which
  checkout keeps to which engine.

## Consequences

- The engines' endpoints can change without notice: a reader's real contract
  run is what tells, and the limit is the fallback.
- Guild's ledger crate and factorium's account code are removed once their
  callers use stablehand.
