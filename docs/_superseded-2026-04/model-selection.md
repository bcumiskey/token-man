> **SUPERSEDED — HISTORICAL ONLY. DO NOT USE AS GUIDANCE.**
> Archived 2026-09-02. Prices, model names and the cache multiplier in this file
> are from April 2026 and are wrong. The live version of this material is the
> `token-management` skill on the account. Kept for history only.

# Model selection

Decision tree for choosing between Opus, Sonnet, and Haiku across the Claude 4.x family. Current as of April 2026.

## The models

- **Haiku 4.5** — fastest, cheapest, 200K context. Default for tool-calling glue, formatting, simple edits, classification, routing decisions. Claude for Chrome is Haiku-locked.
- **Sonnet 4.6** — the workhorse. 200K context standard, 1M GA since March 2026 (Enterprise and API). Best coding model per Anthropic's own positioning; appropriate for 80% of real work.
- **Opus 4.6** — premium reasoning, 200K/1M. Strongest on novel architecture, hard debugging, nuanced writing. Expensive; use deliberately.
- **Opus 4.7** — released April 16, 2026. Strongest model currently available. **Uses a new tokenizer that maps the same text to 1.0–1.35× more tokens than 4.6 at unchanged list pricing.** Factor this into cost comparisons — a 4.7 run "priced the same" as 4.6 can cost 35% more for the same work.

## Decision tree

```
Is the task primarily formatting, glue code, or tool orchestration?
├── Yes → Haiku 4.5
└── No ↓

Does the task need novel reasoning, or is it an expensive-if-wrong decision
(architecture, security, ambiguous refactor affecting many files)?
├── Yes ↓
│   Is cost a hard constraint?
│   ├── Yes → Sonnet 4.6 with plan mode, escalate to Opus only on sticking points
│   └── No  → Opus 4.6 (stable tokenizer) or 4.7 (stronger, watch cost)
└── No → Sonnet 4.6
```

## Subagent defaults

- Research/exploration subagents → **Haiku 4.5**. They read a lot, return summaries; Haiku is plenty.
- Code-writing subagents working on isolated modules → **Sonnet 4.6**.
- Critic or reviewer subagents on critical-path code → **Opus 4.6**, and only for the review turn.

Declare `model: haiku` explicitly in `.claude/agents/<agent>.md` frontmatter. Subagents inherit the parent model otherwise, which silently escalates costs.

## Routing patterns

- **Aider-style two-model**: Opus or Sonnet proposes, Haiku applies diffs. Good pattern for code-heavy sessions; claude-code-router supports this via task-type routing.
- **Plan with Opus, execute with Sonnet**: start a complex feature in plan mode on Opus, switch to Sonnet for implementation.
- **Draft with Haiku, polish with Sonnet**: prose workflows. Haiku generates, Sonnet edits.

## Cost anchors (approximate, April 2026)

Always verify current pricing at platform.claude.com — this is for sanity-check magnitudes only:

- Haiku 4.5: roughly 1/10th the cost of Sonnet per token
- Sonnet 4.6: the reference point
- Opus 4.6: roughly 5× Sonnet per token
- Opus 4.7: same list price as 4.6 but 1.0–1.35× more tokens for identical text

Cached reads are 0.1× the base input rate regardless of model, so cache-hit rate matters more than model choice for repetitive workflows.

## Common miscalibrations

- **Using Opus for boilerplate** — wastes money; Sonnet or Haiku is indistinguishable on this work.
- **Using Haiku for novel debugging** — wastes time; you'll iterate more and may end up escalating anyway.
- **Defaulting subagents to the parent's model** — silent cost multiplier; most subagent work is Haiku-appropriate.
- **Comparing Opus 4.7 cost to 4.6 at face value** — forgetting the tokenizer change.
