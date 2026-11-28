# 0001. One home for agent accounts: logins and usage together, readers per engine

Status: accepted, 2026-11-24 (Felix, control session); the command an engine starts with is stablehand's since 0003

## Context

An agent account's facts had four homes: mytokens' `agent-account-*`
entries, factorium's account code, Guild's ledger crate, and afkconvoy's
window readers, each reading a different part. On 2026-11-24 every engine
Felix uses answered its usage on this Mac, live, with no model turn
(`src/readers/`), by endpoints none documents for outside use.

## Decision

- **stablehand is the one home** of an account: its engine, its login, where
  it may run, its terms, its readings and its off switch. mytokens'
  `agent-account-*` entries move here and go.
- **An account is a subscription, not an engine.** Engines that spend one
  account (pi and codex on one ChatGPT account) book into the same account.
- **stablehand runs no login and keeps no secret** (Felix, 2026-11-24). A
  person logs an engine in by the engine itself. A bound account is logged
  in when its home holds its engine's login file; a portable one when the
  command it names prints its token, run each time, the token written
  nowhere. A refusal of an account not logged in names the line that logs
  it in.
- **One reader per engine**, the only code that knows its endpoint, held to
  the real engine by a truefit contract. A reader that fails leaves the
  windows as last read, stale, and a caller's limit stands in.
- **Reading is bounded**: at most once an hour per account unless a caller
  asks, never again on a credential the engine refused (dsh deletes its
  grant on a 401) until a login after it.
- **stablehand the login, the caller the session.** `stablehand credential`
  answers an account's login and nothing else: a portable token, or a bound
  account's login files. The caller links only those into the session's
  home, so no person's settings, hooks or skills reach a session.
- **The pick is stablehand's, and so is the order** (Felix, control
  session, 2026-11-24): what the next session starts on (engine, model,
  account) is the first of the order whose account takes one more session,
  given the stop percent, the pace and the sessions running. Every caller
  (factorium, a person, Guild) gets one answer; the session itself stays
  the caller's.

## Consequences

- The engines' endpoints can change without notice: a reader's real contract
  run is what tells, and the limit is the fallback.
- Guild's ledger crate and factorium's account code are removed once their
  callers use stablehand.
