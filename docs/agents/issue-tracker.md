# Issue tracker: factorium

Issues for this repo live in factorium's local tracker on this host, not in
GitHub Issues: factorium does not read GitHub. `<checkout>` is this repo's
name in `factorium checkout list` (`stablehand`).

- Read: `factorium issue list stablehand`, `factorium issue show stablehand <n>`
- File: `factorium issue add stablehand <title> --body-file - [--blocked-by <n> --why <text> …]`.
  Give dependencies when filing: an issue with no blocker is ready, and a
  worker takes it on the next tick.
- Comment: `factorium issue comment stablehand <n> …`
- Dependencies: `factorium issue block stablehand …`

There are no labels: whether an issue can start is decided only by its
blockers.
