> **SUPERSEDED — HISTORICAL ONLY. DO NOT USE AS GUIDANCE.**
> Archived 2026-09-02. Prices, model names and the cache multiplier in this file
> are from April 2026 and are wrong. The live version of this material is the
> `token-management` skill on the account. Kept for history only.

---
name: token-management
description: Diagnose and resolve token usage, context-window, and cost problems across the Claude ecosystem. Use this skill whenever the user mentions tokens, cost, billing, context window, context rot, compaction, /compact, /clear, /context, /cost, ccusage, statusline, cache hits, cache misses, prompt caching, rate limits, usage limits, the 5-hour window, "am I going to hit my limit", "why is this so expensive", subagent costs, Opus vs Sonnet vs Haiku selection, model routing, skill bloat, MCP tool bloat, Cowork cost, or any variant of "I'm running out of context" / "session is getting slow" / "Claude keeps forgetting". Also use proactively when a Claude Code session appears to be deep (many tool calls, long conversation, or the user mentions the session has been running a while) and cost/context implications would inform the response. Covers Claude Code, claude.ai, Cowork, Chrome, Excel, and direct API workloads.
---

# Token Management

A diagnostic and intervention skill for the integrated problem of context-window engineering plus API-level cost/token accounting across Anthropic's product surfaces. Grounded in the operating model that **Claude Code is the instrumented workbench and the other surfaces (claude.ai, Cowork, Chrome, Excel) are uninstrumented endpoints** — the discipline is to push real work toward the instrumented surface and treat the rest as specialized tools with known cost opacity.

## When this skill fires

Invoke when the user raises any of:

- **Symptoms**: slow responses, "context is full", Claude forgetting earlier instructions, repeated re-reading of the same files, unexpectedly large bills, approaching a rate limit, subagent costs ballooning.
- **Planning**: choosing a model, deciding whether to use subagents, setting up caching, budgeting a long session, architecting an agentic workflow.
- **Tooling**: ccusage, ccstatusline, hooks, skills, MCP servers (especially Tool Search, Serena, mcp-memory-service), claude-code-router, Admin API, OpenTelemetry.
- **Surface-specific**: Cowork task sizing, Chrome Haiku behavior, Excel Sonnet workloads, claude.ai long conversations, Claude Code `/context` and `/cost` output.

If the user's question is purely "how do I use feature X" and not about managing what it costs or what it consumes, defer to the relevant Anthropic documentation rather than this skill.

## The four-layer model

Every token-management problem lives in one of four layers. Identify which before intervening.

1. **Measurement** — Do we know what's happening? (ccusage, /context, /cost, OTel, Admin API)
2. **Budget** — Have we set limits, and are we inside them? (session budgets, subagent caps, plan windows)
3. **Optimization** — Are we spending the tokens we do spend well? (caching, progressive disclosure, model selection, subagent isolation)
4. **Governance** — Is this sustainable across Anthropic's release velocity? (weekly review, version pinning, release-note triage)

A surprising number of "token problems" are really measurement problems — the user can't see what's happening and has no baseline to compare against. Always start by asking what visibility they have.

## Operating model — which surface are they on?

Different surfaces expose wildly different instrumentation. Internalize this table:

| Surface | `/context` | `/cost` | Cache metrics | OTel | Programmatic usage |
|---|---|---|---|---|---|
| Claude Code (CLI/IDE) | Yes | Yes (API billing; not Pro/Max) | Yes | Yes (opt-in) | ccusage/ccstatusline/Admin API |
| claude.ai chat | No | No | No | No | Admin API (teams) + browser ext |
| Cowork | No | No | No | Team/Enterprise only | Admin API (teams) only |
| Claude for Chrome | No | No | No | No | None (Haiku-locked) |
| Claude for Excel | No | No | No | No | Admin API (teams) only |
| Direct API | N/A | Response headers | `usage` block | Yes | Admin API / count_tokens |

If the user is asking about cost on Cowork, Chrome, Excel, or claude.ai, be honest: the only real levers are the Admin API (teams/enterprise) and the browser extension for claude.ai. Recommend doing cost-sensitive work in Claude Code where you can see it.

## Triage workflow

When the user reports a token problem, walk these steps in order. Don't skip ahead.

### Step 1 — Establish ground truth

Ask or infer:
- Which surface? (Claude Code, Cowork, etc.)
- Billing mode? (Pro/Max subscription vs. API credits vs. Enterprise)
- Is this a single session problem or a pattern over days/weeks?

If they're in Claude Code, direct them to run `/context` and `/cost` *before* anything else. If they don't have `ccusage` installed and they're on Claude Code, that is almost always the right first recommendation — it's `npx ccusage@latest` and takes under a minute.

### Step 2 — Classify the problem

Map the symptom to one of these categories:

- **Context-full / degradation** → Optimization layer. Compaction, just-in-time loading, subagent delegation.
- **Unexpected cost / high bill** → Measurement + Optimization. Find the leak first (usually: large file reads, uncached repeated context, wrong model, runaway subagents), then fix it.
- **Approaching rate limit** → Budget layer. Short-term: model routing, `/clear` discipline. Long-term: version the workflow to reduce re-reads.
- **"Claude forgets things"** → Optimization. Structured note-taking (TodoWrite, plan.md, memory tool), not bigger context.
- **Skill or MCP bloat** → Optimization. Enable Tool Search, audit installed skills, check the `/context` grid for "excluded due to budget".
- **Cross-surface confusion** → Measurement. Explain the visibility gaps honestly; don't pretend Cowork exposes what it doesn't.

### Step 3 — Apply the intervention

See "Interventions" below. Match intensity to severity: a session at 40% context doesn't need `/compact`, it needs discipline. A session at 90% needs immediate action.

### Step 4 — Install the prevention

Every intervention should end with a structural fix so the same problem doesn't recur next week. If you `/clear` a session and move on, you've treated the symptom. If you add a `.claudeignore`, a hook, or a skill, you've treated the cause.

## Interventions by severity

### Light-touch (session at <60% context, costs are normal)

- Keep going. Encourage `/context` checks every ~20 turns as situational awareness, not ritual.
- If they're about to start a new task, recommend `/clear` between unrelated tasks — cheaper than compaction because it preserves the prompt cache on stable preamble.
- Verify cache is working: a healthy Claude Code session shows `cache_read_input_tokens` much larger than `cache_creation_input_tokens` in ccusage output.

### Medium (60–80% context, or noticeable cost creep)

- Check `/context` grid. If skills or MCP tools are consuming a disproportionate slice, that's the fix — not compaction.
- If they haven't enabled `ENABLE_TOOL_SEARCH=auto:5` and run 3+ MCP servers, enable it. See `references/surface-map.md` for the full env-var list.
- Offer subagent delegation for the next tool-heavy subtask (research, codebase exploration). Subagents run in isolated windows and return compressed summaries. Caveat this with the 15× cost-multiplier warning for multi-agent workflows — subagents are a context tool, not a cost tool.
- Persist state to `plan.md` or `NOTES.md` so a `/clear` is non-destructive. Claude is less likely to overwrite JSON than markdown, so prefer JSON for mutable status files.

### Heavy (>80% context, or active cost emergency)

- **If cost emergency**: stop the session. Run `ccusage` to find the specific culprit window. The most common causes, in order: large file reads without `.claudeignore`, runaway subagent chains, wrong model for the task (Opus on boilerplate), broken cache (check for invalidators in `references/cache-strategy.md`), uncaught infinite loop.
- **If context emergency**: `/compact` with focused instructions (`/compact focus on the auth refactor, drop unrelated exploration`). Default `/compact` without focus is lossy. Better still: persist to `plan.md`, `/clear`, reinject.
- **If recurring pattern**: this is now a governance problem. Schedule a weekly ccusage review and a `.claudeignore` audit. See `references/compaction-playbook.md`.

### Structural (prevention layer)

- Add/audit `.claudeignore` — the single highest-ROI fix in the ecosystem.
- Install `ccusage` and `ccstatusline` if not already present.
- If running API workloads, turn on OpenTelemetry: `CLAUDE_CODE_ENABLE_TELEMETRY=1` plus standard OTLP vars.
- If the user has a stable workflow, codify it as a CLAUDE.md plus a `SessionStart` hook that injects the current `plan.md`.
- Consider Serena MCP for any nontrivial codebase — symbol-level retrieval instead of whole-file reads.

## Model selection shortcut

Full decision tree is in `references/model-selection.md`. Quick heuristic:

- **Boilerplate, formatting, simple edits, tool-calling glue** → Haiku 4.5
- **Most coding, refactoring, analysis, writing** → Sonnet 4.6
- **Hard reasoning, architecture decisions, novel problems, code review on critical paths** → Opus 4.6 or 4.7

Opus 4.7 uses a new tokenizer that inflates token counts 1.0–1.35× vs. 4.6 at identical list prices — factor this in when comparing costs. For subagents, default to Haiku unless the subtask genuinely needs more.

## Cache-aware prompt structure

If the user is building on the API (not just using Claude Code), cache placement is the highest-ROI optimization available. Short version:

- Order content **static-first, dynamic-last**: system prompt → tools → stable docs (CLAUDE.md, skill bodies, RAG context) → conversation → current message.
- Place `cache_control` breakpoints on the **last stable block**, never on changing content.
- A 20-turn, 100K-context session drops from ~$6 without cache to ~$0.95 with proper cache placement.
- Full invalidator list and the 5-minute / 1-hour TTL tradeoffs are in `references/cache-strategy.md`.

## What not to do

- Don't reflexively recommend `/compact`. It breaks the prompt cache; `/clear` plus reinject from notes is usually cheaper.
- Don't suggest bigger context windows as a fix for context rot. All 18 frontier models in the Chroma study degraded with length, continuously. Effective windows are ~50–70% of advertised.
- Don't recommend multi-agent workflows for cost savings. Subagents reduce *context* pressure but increase *token* spend (roughly 15× per Anthropic's own docs).
- Don't claim features that don't exist. Cowork has no `/cost`. Chrome has no token UI. Be honest about this; the user's frustration at opacity is warranted and validating it builds trust.
- Don't overfit to any single community benchmark. Check methodology; many published "X% token reduction" claims don't survive scrutiny (notably the SuperClaude 70% figure, computed from word count).

## Communication style

Users asking about token management are often frustrated — the release cadence genuinely has outpaced documentation, and they've probably already been through one round of confusing advice. Be direct, concrete, and specific. Numbers beat adjectives. Name tools, commands, and files. If something isn't in their control (Cowork opacity, subscription window), say so plainly rather than offering vague optimizations that won't help.

## References

Load these as needed — don't load all of them preemptively.

- `references/model-selection.md` — full decision tree for Opus/Sonnet/Haiku including subagent defaults and the Opus 4.7 tokenizer note
- `references/compaction-playbook.md` — decision tree for `/clear` vs. `/compact` vs. subagent handoff vs. memory tool, with examples
- `references/cache-strategy.md` — prompt-caching placement, invalidator list, TTL tradeoffs, Batch API stacking
- `references/surface-map.md` — per-surface instrumentation inventory, env vars, and ceiling behaviors (5-hour window, weekly limits, 1M context betas)
- `references/community-tools.md` — install recipes for ccusage, ccstatusline, Serena, mcp-memory-service, claude-code-router, with the honest tiering (mature / working / hype)
