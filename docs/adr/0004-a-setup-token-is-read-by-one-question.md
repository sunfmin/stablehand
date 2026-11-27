# 0004. A setup token is read by one question, at most once an hour

Status: accepted, 2026-11-24 (built with the claude-code reader); recorded
2026-11-26.

## Context

claude-code's usage endpoint (`GET api.anthropic.com/api/oauth/usage`)
refuses a `claude setup-token` token (HTTP 403 scope), so a portable
claude-code account had no windows in the books. One turn of Claude Code
answers them: its stream's `rate_limit_event`. A turn spends the
account's quota and money, so reading this way is worth it only if a
turn is small enough to ask every hour.

## Decision

- Where the endpoint fails in any way (any non-2xx status, an answer with
  no window, curl failing), the account is asked the question instead in
  the same read, one turn on the smallest model with no tools, settings,
  servers or kept session (`QUESTION` in `src/readers/claude_code.rs`), in
  a new empty config dir removed after. The read fails only if both fail,
  saying both.
- Only a token the endpoint refuses its scope (HTTP 403
  `oauth_scope_insufficient`) is asked the question again, not its usage,
  once read so. Any other failure (a 429, a 5xx) is temporary: the next
  read asks usage first again, since staying on the question would spend
  a turn an hour on a token usage reads for free. Revisited 2026-11-27,
  when usage answered a portable token HTTP 429 (`retry-after` about an
  hour) three reads running while the question read its windows.
- It is asked at most once an hour (`stablehand read`'s wait), unless
  `--now`; the status line starts one such read at most every ten
  minutes, so at most about 6 an hour while its bars refresh.
- Each read says `read_by` and `spent_turn`, so a caller sees the spend.

## Evidence

The question costs about 3 s and under $0.0002 a time (Claude Code
2.1.326, as afkconvoy's `WINDOWS_QUESTION` recorded it). Measured again
by the stream's `result` event, with a setup token in
`CLAUDE_CODE_OAUTH_TOKEN`:

```sh
CLAUDE_CONFIG_DIR=$(mktemp -d) claude -p --model haiku --output-format stream-json \
  --verbose --tools '' --strict-mcp-config --no-session-persistence \
  --setting-sources '' --system-prompt 'Answer with one word.' quota \
  | jq -c 'select(.type == "result") | {duration_ms, total_cost_usd}'
```

## Consequences

- A portable claude-code account is paced and picked like a bound one,
  at a cost of one smallest-model turn an hour of its quota.
- Should the question cost more (a model change, a pricing change), the
  measurement above is how to see it, and the hourly bound is what to
  change.
