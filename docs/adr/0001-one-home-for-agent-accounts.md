# 0001. One home for agent accounts: logins and usage together, readers per engine

Status: accepted, 2026-11-24 (Felix, control session)

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
  it may run, its terms (sessions at once, a token's expiry), its readings
  and its off switch. mytokens' `agent-account-*` entries move here and go;
  factorium asks stablehand for a pick and an account's credential.
- **An account is a subscription, not an engine.** Engines that spend one
  account (pi and codex on one ChatGPT account) read and book into the same
  account.
- **Logins are the engine's own.** stablehand runs the engine's login
  command (`claude setup-token`, `codex login`, `grok login`, …) in that
  account's home; it never runs a vendor's OAuth client itself. A portable
  token is kept in sesame, reached through sesame's command line (`sesame
  login <p> --token`, `sesame token <p> --json`), not linked as a crate.
- **One reader per engine**, the only code that knows its endpoint or
  command, each held to the real engine by a truefit contract. A reader that
  fails leaves the account's windows as last read, stale, and a caller's
  `limit` stands in until it reads again.
- **Reading is bounded**: at most once an hour per account unless a caller
  asks, never again on a credential the engine refused (dsh deletes its grant
  on a 401).
- **stablehand the login, the caller the session.** `stablehand credential`
  answers an account's login and nothing else of it: a portable token, or a
  bound account's engine login files with where the engine's home holds them.
  The session's home is the caller's own, which links only those files into
  it, so nothing else of the account's home (a person's settings, hooks,
  skills) reaches a session. Each engine's login file is listed once, here:
  what a caller links is what makes a bound home logged in.
- **The pick is stablehand's**: the stop percent, the pace and the sessions
  already running decide it here, so no caller computes it again.

## Consequences

- The engines' endpoints can change without notice: a reader's real contract
  run is what tells, and the limit is the fallback.
- Guild's ledger crate and factorium's account code are removed once their
  callers use stablehand.
