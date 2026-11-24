# Glossary

- **Engine**: an agent program a session runs in: `claude-code`, `codex`,
  `pi`, `kimi`, `agy`, `grok`, `dsh`. Each has one **reader** and one
  **login** in stablehand.
- **Account**: one subscription (or one pay-as-you-go wallet) that sessions
  spend, named by the person (`claude-felix`). Not an engine: two engines may
  spend one account (pi and codex on one ChatGPT account), and one engine may
  have several accounts.
- **Register**: the accounts stablehand keeps, with their terms (sessions
  at once, a portable token's expiry and token command) and whether each is
  off; one SQLite file, holding no token.
- **Login**: how an account's credential is held; stablehand runs none and
  keeps no secret. **Portable**: a long-lived token (`claude setup-token`)
  kept where the person keeps secrets (the Keychain, 1Password, mytokens);
  stablehand keeps only its **token command**, which prints it, and the
  account is logged in when that answers. **Bound**: the engine's own login
  files in a home of that account's on one machine (`auth.json`,
  `~/.gemini`), written and refreshed by the engine itself; a home holding
  its engine's login file is logged in, as of when it was written.
- **Identity**: the subscription a bound login's files say they are of
  (ChatGPT's account id, Claude's account and organization): what tells
  two logins of one account apart from two accounts. `account scan` reads
  it in memory and keeps only its hash; a portable token tells none. Two
  logins of one identity are **copies**: one is the account, the others are
  left unused, since copies of one refresh token refresh each other out.
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
  resets (**tight**), resets with much left (**spare**), or is **used up**.
- **Pick**: the answer to "which account may one more session start on now":
  an account, or none with until when and why.
- **Off**: an account the person switched off; nothing is picked on it.
