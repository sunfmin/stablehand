# Glossary

- **Engine**: an agent program a session runs in: `claude-code`, `codex`,
  `pi`, `kimi`, `agy`, `grok`, `dsh`. Each has one **reader** and one
  **login** in stablehand.
- **Account**: one subscription (or one pay-as-you-go wallet) that sessions
  spend, named by the person (`claude-felix`). Not an engine: two engines may
  spend one account (pi and codex on one ChatGPT account), and one engine may
  have several accounts.
- **Register**: the accounts stablehand keeps, with their terms (a
  portable token's expiry and token command) and whether each is
  off; one SQLite file, holding no token.
- **Login**: how an account's credential is held; stablehand runs none and
  keeps no secret. **Portable**: a long-lived token (`claude setup-token`)
  kept where the person keeps secrets (the Keychain, 1Password, optokens);
  stablehand keeps only its **token command**, which prints it, and the
  account is logged in when that answers. **Bound**: the engine's own login
  files in a home of that account's on one machine (`auth.json`,
  `~/.gemini`), written and refreshed by the engine itself; a home holding
  its engine's login file is logged in, as of when it was written.
- **Identity**: the subscription a login says it is of (ChatGPT's account
  id, Claude's account and organization): what tells two logins of one
  account apart from two accounts. Kept only as its hash; how `account
  scan` reads it is in its help.
- **Organization**: a portable claude-code token's identity, named by every
  answer to it. A personal one (`claude_max`, `claude_pro`) is its
  subscription; a team's is several people's seats.
- **Copy**: a login of an account's identity, or a token of a bound login's
  personal organization: it spends the account's windows, so it is left
  unused (copies of one refresh token refresh each other out).
- **Window**: one span of an account's use as its engine names it
  (`five_hour`, `seven_day`, `primary`, `gemini-5h`): how much is used, 0 to
  100, and when it resets. An account may have several **groups** of windows
  (agy's per model group, Grok's per product); a session of a model spends
  only the windows of the group holding it, and those in no group.
- **Balance**: what is left of a pay-as-you-go account (dsh), in its
  currency. Such an account has no windows.
- **Reading**: what one ask of an engine said of an account at a time: its
  windows or balance. Kept in the **books**, newest last.
- **Reader**: the one piece of stablehand that asks one engine for a reading
  and turns its answer into windows; the only code that knows the engine's
  endpoint or command.
- **Limit**: a caller's report that a session met its account's limit, with
  when it lifts where the engine said: the reading of an engine whose reader
  failed.
- **Pace**: per window, whether at its recent rate it runs out before it
  resets (**tight**), resets with much left (**spare**), or is **used up**;
  **unknown** where the account's newest reading is **old**, over a day.
- **Order**: the engines, and of each the models, the next session starts
  on, highest first (`claude-code@opus codex grok agy@gemini-4-neon-high,claude-opus-5-5-high`);
  one per machine, kept beside the accounts. A preference only: the
  command an engine starts with is its kept command's, its guard the caller's.
- **Pick**: the answer to "what may one more session start on now": the
  first engine, model and account of the order whose account takes it,
  or none with until when and why. The session itself is the caller's.
  A copy gives way to its account while that one is on
  here: one subscription takes sessions under one name.
- **Kept command**: how this host starts an engine, its program and the
  flags it always takes (`claude --dangerously-skip-permissions`), one per
  engine, kept beside the accounts (`stablehand command`); `stablehand run`
  starts it, its own arguments after the kept ones. None kept: the bare
  program. The session's own arguments, home and protocol are the caller's.
- **Session**: one run of an engine on an account. The caller's, but one
  started by `stablehand run` is kept in the register while it runs, and
  counted by a pick.
- **Off**: an account the person switched off; nothing is picked on it.
