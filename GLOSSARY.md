# Glossary

- **Engine**: an agent program a session runs in: `claude-code`, `codex`,
  `pi`, `kimi`, `agy`, `grok`, `dsh`. Each has one **reader** and one
  **login** in stablehand.
- **Account**: one subscription (or one pay-as-you-go wallet) that sessions
  spend, named by the person (`claude-felix`). Not an engine: two engines may
  spend one account (pi and codex on one ChatGPT account), and one engine may
  have several accounts.
- **Register**: the accounts stablehand keeps, with their terms (sessions
  at once, a portable token's expiry) and whether each is off; one SQLite
  file, holding no token.
- **Login**: how an account's credential is held. **Portable**: a long-lived
  token (`claude setup-token`) kept in sesame, usable on any machine that
  reaches sesame's store. **Bound**: the engine's own login files in a home of
  that account's on one machine (`auth.json`, `~/.gemini`), refreshed by the
  engine itself.
- **Window**: one span of an account's use as its engine names it
  (`five_hour`, `seven_day`, `primary`, `gemini-5h`): how much is used, 0 to
  100, and when it resets. An account may have several **groups** of windows
  (agy's per model group, Grok's per product).
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
