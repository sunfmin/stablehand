# 0005. A pick spends first the quota that resets soonest

Status: accepted, 2026-11-28 (Felix: "the pick should spend first the quota
that is about to be lost")

## Context

The pick took, of the accounts that may take a session, the one with the
most left of its most used window. An account that reset yesterday won, and
one resetting tomorrow with 15% left was never used: that 15% was lost at
its reset. Seen on the claude-code / opus step: 15% left resetting in 1d 2h,
25% in 2d 12h, 84% in 6d 6h; the pick answered the last.

## Decision

- Within one step of the walk (one engine and model, its accounts
  interchangeable), the account whose long window resets first is picked.
  Of equal resets, and of those whose long window's reset is not known
  (ranked after every one known), the one with the most left, as before.
- An account's long window is its longest by length (each reader gives the
  length its engine or the window's name says), where it has a shorter one
  too. A sole window, or windows of one length, may be the short one: such
  an account has none, and is ranked by what is left.
- The order across engines and models stays the owner's: a lower step's
  quota expiring soon goes ahead of no higher step.
- A window that at its rate runs out before it resets still passes an
  account over for a short window (a session cut off mid-work), never for
  its long one: running the long window down just before its reset is the
  aim, and the stop percent still holds a margin.
- A long window within 12 hours of its reset stops at 95% used, not at
  the stop percent (a stop percent above 95 holds). Felix, 2026-11-28:
  the stop percent's margin is for sessions already running; a few hours
  before a long window resets there is little to protect, and up to 10%
  of every week was lost at its reset. Every other window, and a long one
  further from its reset, stops at the stop percent.
- A copy is still passed over while its account is on here.

## Consequences

- An account whose week is nearly spent and resets soon takes sessions
  until it stops, then the next to reset; the one with most left, resetting
  last, waits.
- An engine whose reader gives one window, or no lengths, is picked by
  what is left, as before.
