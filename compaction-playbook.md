# Compaction playbook

Decision tree for deciding among `/clear`, `/compact`, subagent handoff, and memory-tool persistence. The instinct to reach for `/compact` as a default is usually wrong — it breaks the prompt cache, and the summarization itself costs tokens.

## The four mechanisms

| Mechanism | What it does | Cache impact | Loss | When |
|---|---|---|---|---|
| `/clear` | Resets conversation, keeps CLAUDE.md and skills | Preserves cache on preamble | Everything in conversation | Task boundary, unrelated next task |
| `/compact [focus]` | Summarizes conversation, keeps summary | Breaks cache | Detail loss proportional to compression | Mid-task when context is full but you need continuity |
| Subagent handoff | Delegates subtask to isolated context, gets summary back | Preserves parent cache | Only the compressed summary returns | Bounded subtask (research, exploration, isolated refactor) |
| Memory tool + `clear_tool_uses` | Persists state to memory, clears old tool results | Partial cache preservation | Tool results replaced with placeholders; memory survives | Long-running agent loops with repeated tool calls |

## Decision tree

```
Is the next task related to the current one?
├── No → /clear, then start fresh. Cheapest option.
└── Yes ↓

Can you extract the state you need into plan.md, NOTES.md, or a JSON status file?
├── Yes → Persist state, /clear, reinject the file. Near-zero information loss, full cache preservation.
└── No ↓

Is the bloat primarily from tool results (many Read/Grep/Bash outputs)?
├── Yes → Use context editing (`clear_tool_uses_20250919`) if on API, or /compact with
│         "preserve plan and recent decisions, drop old tool outputs" focus.
└── No ↓

Is the next sub-step genuinely bounded (e.g., "search the codebase for X")?
├── Yes → Spawn a subagent. Parent keeps clean context, gets summary back.
└── No → /compact with explicit focus. Never /compact without focus instructions.
```

## Focus-instruction patterns for /compact

Default `/compact` is lossy. Give it direction:

- `/compact focus on the authentication refactor; drop exploratory tangents`
- `/compact preserve the plan and open TODOs; summarize completed work in one line each`
- `/compact keep decisions and their rationale; drop intermediate tool outputs`

## The "write it down" principle

The top community wisdom on this, distilled:

> If context is important, do not trust it to the conversation history; make sure it's written down.

Operationally:
- Use TodoWrite for tracking work-in-progress items.
- Keep a `plan.md` at repo root with the current intent and open questions.
- Keep a `NOTES.md` or `progress.json` with decisions and their rationale.
- `SessionStart` hook can auto-inject these on resume.

This converts "compaction" from a lossy compression problem into a lossless persistence problem. It's the single biggest behavior change that separates users who fight context from users who manage it.

## API-level: context_editing + memory tool

For direct API users building agents:

- Enable `context_management.edits: [{type: "clear_tool_uses_20250919", ...}]` to automatically replace old tool results with placeholders when context fills.
- Enable the memory tool (`memory_20250818`) for durable state.
- Set `exclude_tools: ["memory"]` on the clear operation so memory ops survive compaction boundaries — otherwise you lose the very thing you're persisting to.
- Anthropic reports 84% token reduction and 39% quality lift on a 100-turn web search eval with this combo.

## What /compact is bad at

- Preserving exact quotes, code snippets, or structured data
- Preserving numerical precision (counts, timestamps)
- Preserving order of events when order matters
- Cache-hit rate (it breaks cache)

If any of those matter for your next turn, don't compact — persist and reload.

## Recovery patterns

If a session has gone too far:

1. Ask Claude to write a "session summary" into `plan.md` before anything else — capture state while it's still accessible.
2. Ask Claude to note open questions and assumptions.
3. `/clear`.
4. Start the next turn with "Read plan.md and continue from where we left off."

This is slower than `/compact` for one cycle but preserves cache for subsequent cycles and is strictly lossless for the state you wrote down.
