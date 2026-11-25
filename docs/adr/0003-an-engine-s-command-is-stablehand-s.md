# 0003. An engine's command is stablehand's; the rest of the session the caller's

Status: accepted, 2026-11-25 (stablehand#30; Felix). Supersedes 0001's "the
command an engine starts with, its flags" as the caller's.

## Context

How this host starts each engine, its program and the permission flags
wanted on every session (`claude --dangerously-skip-permissions`, `codex
--dangerously-bypass-approvals-and-sandbox`, `agy
--dangerously-skip-permissions`, `grok --always-approve`), was kept in
factorium's kit, while `stablehand run` started each engine's bare program
(`Engine::start`). Two homes for one fact: a session started by run and one
started by factorium on the same engine ran with different permissions.
stablehand already says which engine, model and account a session starts
on; how the engine is started on this host is the same kind of fact.

## Decision

- Each engine's **kept command** is stablehand's: one per engine per
  machine, in the register's database (`engine_commands`, its words as
  JSON). `stablehand command set <engine>=<command> …` keeps those named
  (`<engine>=` drops one), `stablehand command show` answers them. A
  command is split as `sh` splits it and never run through a shell; its
  program is its engine's or a path ending in it, and it names no model
  (the model is the pick's). Anything else is refused (`bad_command`),
  writing nothing.
- A fresh store keeps none: full permissions are a choice each person
  makes, not stablehand's default. Each engine starts bare until one is
  kept.
- `stablehand run` starts the kept command: with no command, as it is; a
  command of the engine's program gets the kept arguments right after its
  program, before its own, and the kept program's path where it names the
  bare program; then the model flag as before. `--bare` leaves the kept
  command out for one run; a command of another program (a wrapper
  `--engine` names) gets none. stderr says `, kept command` when it was
  used, and never its arguments.
- The rest of the session is the caller's: its own arguments, its home,
  and its protocol (how it talks to the engine). factorium reads the kept
  commands from here instead of its kit.

## Consequences

- One place to turn full permissions on or off per engine on a host; a
  caller that wants a session without them runs `--bare`, or keeps none.
- A kept argument and a person's own can conflict (both name a flag); the
  engine decides, as it would on a line typed by hand.
- dsh's start line is `dsh tui`: a command kept for it names `tui`
  itself, and `stablehand run dsh tui` gets the kept arguments after `dsh`,
  before the person's `tui`.
