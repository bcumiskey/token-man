# Prompt caching strategy

The single highest-ROI API-level optimization. A 20-turn, 100K-context session drops from ~$6 without cache to ~$0.95 with proper cache placement. Claude Code's team treats cache-hit rate as a SEV-worthy metric; if your cache-read tokens aren't dramatically larger than your cache-creation tokens, something is wrong.

## Mechanics

- Breakpoint declaration: add `"cache_control": {"type": "ephemeral"}` to a content block.
- Up to **4 explicit breakpoints** per request.
- Cached reads cost **0.1× the base input rate**; cache writes cost **1.25×** (5-minute TTL) or **2×** (1-hour TTL).
- Automatic caching (GA Feb 2026) advances the breakpoint forward as conversations grow — on by default for most SDKs.
- Stacks with Batch API (flat 50% discount) to reach roughly **95% off** on cached input for appropriate batched workloads.

## The placement rule

Order content **static-first, dynamic-last**, breakpoint on the last stable block:

```
[system prompt]              ← most stable
[tool definitions]
[skill bodies / CLAUDE.md]
[RAG context / docs]          ← breakpoint here (last stable)
[conversation history]        ← dynamic
[current user message]        ← most dynamic
```

A breakpoint on a changing block writes fresh cache every turn and never reads. This is the single most common cache-configuration mistake.

## Invalidators — things that silently break your cache

- Switching models mid-session (Sonnet → Opus)
- Adding or removing a tool from the tool list
- Changing `tool_choice`
- Timestamps in system prompt (`"Current time: 2026-04-20T14:32:17Z"`) — put these in the user message instead
- Image presence/absence changing between turns
- Non-deterministic JSON key ordering in tool definitions (Swift and Go SDKs; serialize with sorted keys)
- Changing the order of elements in otherwise-identical blocks
- Any edit to content *before* the breakpoint, even if the content at the breakpoint is unchanged

## TTL tradeoffs

- **5-minute ephemeral** (default): cheaper write (1.25×), fine for typical interactive sessions where turns are seconds apart.
- **1-hour ephemeral**: more expensive write (2×) but survives across longer gaps — useful for workflows with human review cycles, slow tool calls, or agents that wait on external systems.

Break-even math: 1-hour TTL pays off if you expect more than one subsequent read within the hour. For any session longer than ~15 minutes with pauses, default to 1-hour.

## Verifying cache is working

In ccusage output, healthy sessions show:

```
cache_creation_input_tokens:   ~small, grows slowly
cache_read_input_tokens:       much larger, grows with every turn
input_tokens (non-cached):     small per turn (just the new user message)
```

If `cache_creation` is comparable to `cache_read`, your cache is thrashing — usually because of an invalidator or a breakpoint placed too late.

## Batch API stacking

For workloads that don't need real-time response (nightly report generation, bulk classification, eval runs):

- Submit as batch: flat 50% discount on input and output
- Stacks with cache: cached reads are 0.1× of the already-50%-discounted rate
- Effective discount approaches 95% for highly-cached batched workloads

Don't use batch for interactive work — turnaround is up to 24 hours.

## Claude Code specifics

Claude Code manages cache for you, but you can still hurt it:

- Avoid editing CLAUDE.md mid-session — it invalidates everything after it
- Avoid adding/removing MCP servers mid-session for the same reason
- `/compact` breaks the cache (that's an expected cost of compaction; factor it in)
- `/clear` preserves cache on the stable preamble (system prompt, tools, CLAUDE.md, skills) — that's why `/clear` is usually cheaper than `/compact`

## The one-line test

If you're ever unsure whether caching is helping: compare your ccusage report's `cache_read_input_tokens` to `input_tokens`. If read is 5–20× larger than raw input, caching is working. If they're comparable, caching is broken or misconfigured.
