# Surface map

Per-surface instrumentation inventory, environment variables, and ceiling behaviors. This is the reference for "what can I actually see and control on surface X?"

## Claude Code (CLI, VS Code, JetBrains, Slack)

**Visibility**
- `/context` — colored grid of context-window usage; flags skills excluded for budget
- `/cost` — session totals, dollar estimates. **Excluded on Pro/Max subscriptions** (deliberate; subscription economics are kept opaque)
- `/compact`, `/clear` — the two compaction levers
- Statusline — JSON protocol exposing `rate_limits.five_hour` since v1.2.80; configure with `ccstatusline`

**Hooks (15+ events)**
- `SessionStart` — inject plan.md, reset accounting
- `UserPromptSubmit` — pre-process prompts, enforce policies
- `PreToolUse` / `PostToolUse` — cap output size, log usage
- `PreCompact` — route to custom summarizer before default fires
- `SubagentStart` / `SubagentStop` — track subagent costs

**Environment variables**
```
CLAUDE_CODE_ENABLE_TELEMETRY=1         # enable OpenTelemetry emission
OTEL_EXPORTER_OTLP_ENDPOINT=...        # your OTLP backend
OTEL_EXPORTER_OTLP_HEADERS=...         # auth
ENABLE_TOOL_SEARCH=auto:5              # progressive MCP tool disclosure (3+ servers)
MAX_THINKING_TOKENS=8000               # cap extended-thinking overhead
DISABLE_EXTRA_USAGE=1                  # hard stop at plan limits; never bill overage
```

**Ceilings**
- Pro: 5-hour rolling window shared with claude.ai
- Max: higher 5-hour window, plus a weekly limit
- API: rate limits by tier; check headers `anthropic-ratelimit-*`

**Programmatic usage**
- `ccusage` parses `~/.claude/projects/*/*.jsonl` — historical trend analysis
- `cccost` instruments `fetch()` in the Node process — more accurate than ccusage
- Admin API `/v1/organizations/usage_report/messages` — team/enterprise aggregate

## claude.ai chat

**Visibility** — none in product. Subscription usage shown only as vague "% of 5-hour window."

**Options**
- Browser extension: `lugia19/Claude-Usage-Extension` for per-message token estimates
- Admin API for Teams/Enterprise plans (chat usage *is* in the API for paid team plans)
- For Pro/Max individual plans, there is no programmatic usage data. None.

**Ceilings** — 5-hour rolling window. Unclear exact token count; Anthropic frames it in messages, but messages with long attachments count more.

## Cowork

**Visibility** — effectively none in product. Settings → Usage shows the shared 5-hour meter and nothing else.

**Reality check** — Cowork is Claude Code inside an Apple Virtualization Framework VM. The underlying session *does* have token accounting, but it's not surfaced. ccusage does not capture Cowork sessions because the JSONL lives inside the VM.

**Options**
- Team/Enterprise: OpenTelemetry export (admin-facing, not end-user)
- Admin API usage reports (aggregate only)
- For individual users: plan in claude.ai, delegate to Cowork only for scoped well-defined tasks

**Ceilings** — shared plan window. Anthropic's own guidance: "Larger multi-file Cowork tasks can consume tokens quickly, so break bigger jobs into smaller runs." Translation: they know it's expensive and they're asking you to self-limit.

## Claude for Chrome

**Visibility** — none. The extension has no token UI.

**Model** — locked to Haiku 4.5. This is an implicit cost cap the user cannot change.

**Ceilings** — counts against the shared 5-hour plan window.

**Security note** — the PromptArmor exfiltration demo showed that page content can hijack Chrome's Claude. Treat any page Claude reads as potentially adversarial; don't point it at untrusted content with sensitive tools enabled.

## Claude for Excel

**Visibility** — none.

**Model** — Sonnet 4.6 with bundled finance skills (DCF, comps, earnings).

**Ceilings** — plan window shared.

**Honest assessment** — the strongest vertical product Anthropic has shipped, but token economics are fully invisible. Fine for exploration; think twice before running it on large workbooks unattended.

## Direct API

**Visibility** — everything, via the `usage` block in every response and the Admin API for aggregates.

**Key endpoints**
- `/v1/messages/count_tokens` — free estimator (not exact billed count)
- `/v1/organizations/usage_report/messages` — aggregate usage by model/workspace/service tier
- `/v1/organizations/cost_report` — dollar costs for chargebacks
- Response headers: `anthropic-ratelimit-requests-*`, `anthropic-ratelimit-tokens-*`

**Ceilings** — tier-based (Tier 1 through Tier 4), plus organization-level spend caps configured in console.

## The fragmentation, summarized

- **Claude Code subscription usage (Pro/Max) is not in the Admin API** — only API-billed usage is.
- **claude.ai chat individual-plan usage is not programmatically exposed anywhere.**
- **Cowork, Chrome, Excel expose no end-user cost UI.**

If you want unified visibility, the only path today is: do cost-sensitive work in Claude Code, use the Admin API for team-level aggregate, and accept that the consumer surfaces are opaque by design.

## What changes in Opus 4.7 era (April 2026)

- New tokenizer: same text maps to 1.0–1.35× more tokens vs 4.6 at identical list pricing
- 1M context on Sonnet 4.6 and Opus 4.6/4.7 (GA for Enterprise and API)
- Legacy 1M beta on Sonnet 4/4.5 retires April 30, 2026
- Release cadence: 33 Claude Code releases in 5 weeks during Feb–Mar 2026; pin versions in production
