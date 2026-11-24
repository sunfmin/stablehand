# stablehand

One home for agent engine accounts: their logins and their usage windows.
See `README.md`; the words are in `GLOSSARY.md`, the decisions in `docs/adr/`.

`./gate.sh` (fmt, clippy, tests via `slimfit check`, the release dry run,
`truefit contract`) must pass before anything lands.

Each change follows `slimfit next`'s loop and the rule for its mutants in
`slimfit help factorium` (run `slimfit help` for the rest). Each engine
reader is tested through a stand-in declared in `truefit.toml`, its real side
run with `truefit contract --real <engine>` and the date committed in
`.truefit/`.

Never print, log or commit a token, a refresh token or an account's email:
tests use stand-ins, and a real run's evidence is the response's shape.

## Agent skills

### Issue tracker

Issues live in factorium's local tracker (`factorium issue … stablehand`). See `docs/agents/issue-tracker.md`.

### Domain docs

single-context: root `GLOSSARY.md` + `docs/adr/`. See `docs/agents/domain.md`.
