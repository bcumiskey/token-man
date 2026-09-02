> **SUPERSEDED — HISTORICAL ONLY. DO NOT USE AS GUIDANCE.**
> Archived 2026-09-02. Prices, model names and the cache multiplier in this file
> are from April 2026 and are wrong. The live version of this material is the
> `token-management` skill on the account. Kept for history only.

# Router design sketch — DEFERRED

**Status:** Design only. Do not implement until after a 2–4 week observation period using the passive tooling (ccusage, ccstatusline, OTel). The point of waiting is to learn what policies you actually want to enforce before introducing a component that sits in the request path.

## Why deferred

A router — `claude-code-router` or equivalent — is the first component in the stack that moves from **observation** to **interception**. Every other tool in the recommended stack degrades gracefully if it breaks: if ccusage breaks, Claude Code still works; if Serena breaks, you fall back to native tools; if hooks break, sessions still run. A router is different — if it breaks or misroutes, you get failed requests, wrong-model responses, or silently worse output. That risk is worth taking only when:

1. You know what you're routing on (task type? context size? cost ceiling? model availability?)
2. You have baseline data to tell whether the router is actually helping
3. You have a fallback path when the router is the problem

None of those are true on day one. All three become true after a few weeks of observation with ccusage and OTel data.

## What the router would do

The core value proposition of `claude-code-router` (musistudio, ~26.4k stars) is **policy-driven request routing** — Claude Code points at the router instead of `api.anthropic.com`, and the router decides which upstream model handles each request based on rules you define. The router is tiktoken-aware, so it can inspect content length before routing.

Typical routing policies:

- **Task-type routing**: background tasks → Haiku, deep-thinking tasks → Opus, default → Sonnet
- **Context-length routing**: <32K → Sonnet, 32–200K → Sonnet with 1M beta, >200K → Opus with 1M
- **Cost-ceiling routing**: first N tokens/day on Sonnet, fall back to Haiku once exceeded
- **Provider routing**: Anthropic for complex reasoning, DeepSeek or local Ollama for boilerplate
- **Failover routing**: Anthropic primary, OpenRouter fallback on rate-limit errors

## Architecture sketch

```
┌──────────────┐   ┌─────────────────┐   ┌──────────────────┐
│ Claude Code  │──▶│  Router (local) │──▶│ api.anthropic    │
│ (IDE/CLI)    │   │                 │   ├──────────────────┤
└──────────────┘   │  - token count  │──▶│ OpenRouter       │
                   │  - task detect  │   ├──────────────────┤
                   │  - policy apply │──▶│ DeepSeek / local │
                   │  - log/meter    │   └──────────────────┘
                   └─────────────────┘
                          │
                          ▼
                   ┌─────────────────┐
                   │ Policy config   │
                   │ + decision log  │
                   └─────────────────┘
```

The router runs as a local proxy (default port 3456). Claude Code's `ANTHROPIC_BASE_URL` points at it. Every request passes through, gets inspected, routed, and logged.

## Policies to define BEFORE enabling

Write these down before touching the install:

1. **What's the default route?** (almost certainly Sonnet 4.6 on Anthropic)
2. **What's the cheap fallback?** (Haiku, DeepSeek, or local model; pick based on quality tolerance)
3. **What's the premium escalation?** (Opus 4.6 vs. 4.7 — remember the 4.7 tokenizer inflation)
4. **Context-size policy**: at what context does routing switch? (Typical: <32K standard, 32–200K standard with caching aggressive, >200K long-context model)
5. **Task-type detection**: how is "background" detected? (System prompt markers? Subagent flag? `thinking` parameter present?)
6. **Cost ceiling**: daily/weekly cap? What happens when hit? (Hard fail? Downgrade to cheapest? Warn and continue?)
7. **Failover**: on rate-limit, fall back where? On upstream error, retry or surface?

If you can't answer most of these from your ccusage data, you're not ready for the router.

## Concrete install recipe (for when you're ready)

```bash
# Install
npm install -g @musistudio/claude-code-router

# Initial config (writes ~/.claude-code-router/config.json)
claude-code-router init

# Start the proxy
ccr start

# Point Claude Code at it
export ANTHROPIC_BASE_URL=http://localhost:3456
export ANTHROPIC_API_KEY=ccr-your-local-token

# Verify
ccr status
```

Config schema is JSON with `providers` (upstream endpoints with credentials) and `routing` (rule list evaluated top-down). See the repo's README for the current schema — it has changed across versions.

## What to watch after enabling

These are the signals that the router is helping, hurting, or neutral:

- **Cost trend** (ccusage daily/weekly): should bend down. If flat or up, routing isn't helping — audit policies.
- **Quality regressions**: subjective but real. Keep a "this felt worse than normal" log for the first week; correlate with router decision logs.
- **Cache-hit rate**: routing across providers breaks cache by definition; routing within Anthropic should preserve it. Watch `cache_read_input_tokens` doesn't collapse.
- **Router uptime**: if the router goes down, Claude Code fails. Set up basic liveness monitoring or at least a one-liner that tests it before each session.
- **Misroute rate**: how often does a task end up on the wrong model? Manual review of the decision log for the first 200 requests tells you whether your task-detection is working.

## Alternatives to a full router

Before committing to a router, consider whether a simpler mechanism solves 80% of the problem:

- **Manual model switching**: `/model` in Claude Code. Cheapest intervention.
- **Subagent model declaration**: `model: haiku` in `.claude/agents/*.md` frontmatter. No router needed; just declare.
- **Task-specific slash commands**: `/research` launches a Haiku subagent; `/review` launches an Opus subagent. Simpler mental model than a router.
- **A hook-based gate**: `PreToolUse` hook that refuses certain tools on Opus, forcing model switches. Lightweight, no proxy.

Often the router ends up doing what a few subagent declarations plus disciplined `/model` usage would have done. The router wins when you have:

- Multi-provider needs (Anthropic + DeepSeek + local)
- Enforcement requirements (budget caps that must be honored)
- Many users whose behavior you can't trust to self-discipline
- Enough volume that manual switching is friction

For a solo developer observing their own usage, subagent declarations and discipline almost always beat a router.

## Decision point — return to this document after observation

After 2–4 weeks with ccusage + OTel, you'll have actual data. At that point, ask:

1. Where's the cost actually going? (Probably not where you guessed on day one.)
2. Which decisions are you making repeatedly that a rule could automate?
3. How often are you switching models manually — often enough that automation pays back?
4. Is there a provider you want to route to that you don't currently use?

If you can answer those with data, design your routing policy from the answers and then install. If you can't, the router isn't the right tool yet.

## Pointer back

When you're ready to revisit this, the relevant references are:

- Observed usage data in ccusage (`ccusage daily --since 30d`)
- Cache-hit patterns in OTel dashboards
- `references/model-selection.md` — the decision tree the router would automate
- `references/surface-map.md` — the env-var list, including `ANTHROPIC_BASE_URL`
- The repo's current schema: https://github.com/musistudio/claude-code-router
