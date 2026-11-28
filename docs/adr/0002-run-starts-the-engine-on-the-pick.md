# 0002. `stablehand run` starts the engine on the pick, and counts its sessions

Status: accepted, 2026-11-24 (Felix, control session; in 0a90b5b)

## Context

A pick (0001) answers an engine, a model and an account; credential answers
the login. A person, a script or factorium then had to put both together
into the engine's command, its environment and home, by hand, every time.
And `pick --running` was the caller's count alone, so a session a person
started by hand was counted by nobody.

## Decision

- `stablehand run [--engine] [--account] [--model] [--home] [--] <command>
  [args…]` picks (the command's program names the engine: `claude` is
  claude-code's) and starts the command on the pick. With no command, the
  whole order is walked and the engine's own program starts, interactive.
  Arguments with no command are refused: each engine reads them otherwise.
- The login reaches the child in its environment alone: a portable token in
  the engine's token variable, a bound home in its home variable (the token
  variable cleared), or `--home <dir>` with the bound login's files linked
  into it. Each engine's program, start line and model flags are one table
  in stablehand (`Engine::start`, `Engine::model_flags`); the model flag is
  added only where the command names no model.
- run waits for the child, no exec: the terminal is shared, its exit status
  is stablehand's (a signal that ends it is raised on stablehand), and a
  signal another process sends stablehand is forwarded. One the kernel sends
  (the terminal's ^C, its hangup: no sending pid) is not, since it already
  reached the child's process group.
- While the child runs its session is a row of the register (`sessions`: its
  pid, account, start), dropped when it ends; a row whose pid is dead (a
  run killed outright) is dropped by the next pick. pick counts these beside
  `--running`.

## What Claude Code does with its login, found on this host (2026-11-24)

Claude Code 2.1.331 on macOS 27.2 (macnoMac-Studio), from its own code (the
installed binary's JavaScript) and the host's state. No login or refresh of
the person's account was forced to find it: a refresh run on a linked copy
would have spent the person's own refresh token.

- **(a) Where `claude auth login` with `CLAUDE_CONFIG_DIR=<dir>` writes.**
  Its secure storage is `keychain-with-plaintext-fallback`: the Keychain
  first, service `Claude Code-credentials-<first 8 hex of
  sha256(<dir>)>` (unsuffixed `Claude Code-credentials` only when
  `CLAUDE_CONFIG_DIR` is unset); `<dir>/.credentials.json` (0600) only when
  the Keychain write fails. It fails where the login keychain is locked to
  the session: an ssh or agent session, where `security show-keychain-info`
  exits 36 ("User interaction is not allowed"), as it does here. So a login
  made at the Mac's own desktop holds no `.credentials.json` (Guild's
  `~/.guild/engines/claude-code` holds neither that nor a Keychain item of
  its hash), and one made from an agent's session holds one:
  `~/.claude/.credentials.json` was rewritten today, while the Keychain's
  `Claude Code-credentials` was last written 2026-11-05. Reading, it tries
  the Keychain, then the file.
- **(b) Whether a linked `.credentials.json` stays a link after a refresh.**
  It does not. Read, a link is followed: `CLAUDE_CONFIG_DIR=<tmp>` with
  `<tmp>/.credentials.json` a link to `~/.claude/.credentials.json` answers
  `claude auth status` `{"loggedIn": true, "authMethod": "claude.ai"}`.
  Written, its file writer opens the target with `O_NOFOLLOW` and writes a
  staging file renamed over the path: the link is replaced by a regular
  file, a copy of the refreshed login, while the linked file keeps the old
  refresh token, spent. Its newer credential store also reads with
  `O_NOFOLLOW` and takes a link as no login (`refused-symlink`).

So `run --home` is refused for a bound claude-code account: its session runs
in the account's own home (`CLAUDE_CONFIG_DIR` set to it). A portable
claude-code account (`claude setup-token`) needs no file and takes `--home`.

## Consequences

- A bound claude-code account whose login is only in the Keychain (logged in
  at the desktop) is not logged in to stablehand, whose test is the file.
  Logging it in from an agent's session writes the file.
- With `CLAUDE_CONFIG_DIR` set to `~/.claude`, Claude Code looks for the
  suffixed Keychain item, not the unsuffixed one, then the file.
- A session started other than by run is counted only by `--running`.
