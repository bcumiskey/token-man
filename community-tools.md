# Community tools

Honest tiering. Star counts and activity are as of April 2026; verify before committing.

## Tier 1 — mature, load-bearing, recommend by default

### ccusage — usage analytics
- **Repo**: `ryoppippi/ccusage` (~10.7k stars, 103 releases)
- **Install**: `npx ccusage@latest`
- **Does**: parses `~/.claude/projects/*/*.jsonl`, reports daily/monthly/session/5-hour-block with separate cache-creation vs. cache-read accounting
- **Caveats**: under-counts slightly because JSONL doesn't capture every API request; for precision, pair with `cccost`
- **MCP server**: ccusage exposes its own MCP server so Claude can query its own usage

### ccstatusline — statusline configurator
- **Repo**: `sirmalloc/ccstatusline` (~7.2k stars)
- **Install**: `npx ccstatusline@latest` (interactive configurator)
- **Does**: writes the correct settings.json entry; surfaces model, git branch, token usage, session duration, 5-hour block timer, context-window bar
- **Why it matters**: continuous awareness is cheaper than periodic `/context` checks

### Claude-Code-Usage-Monitor — real-time TUI
- **Repo**: `Maciek-roboblog/Claude-Code-Usage-Monitor` (~7.6k stars)
- **Does**: terminal TUI predicting when you'll hit Pro/Max limits, with plan-aware ML detection
- **When**: you're on Pro/Max and hit limits more than once a week

### Serena MCP — symbol-level code retrieval
- **Repo**: `oraios/serena`
- **Install**: `uvx --from git+https://github.com/oraios/serena serena start-mcp-server --context claude-code --project $(pwd)`
- **Does**: LSP-based (multilspy/Solid-LSP) symbol lookup across 20+ languages; explicitly instructs models "not to read bodies of code symbols unnecessarily"
- **Why it matters**: whole-file reads are the #1 context-bloat source in codebase work; Serena replaces them with targeted symbol fetches

### Langfuse / LangSmith — OTel-based observability
- **Langfuse**: `pip install langfuse opentelemetry-instrumentation-anthropic` — open-source, self-hostable
- **LangSmith**: the only tool that captures subagent runs as properly nested child spans; pay-as-you-go for individuals
- **When**: production API workloads or serious subagent development

## Tier 2 — working, useful for specific cases

### cccost — precise fetch-level accounting
- **Repo**: `badlogic/cccost`
- **Does**: instruments `fetch()` in the Node process; catches requests ccusage misses
- **When**: you need exact numbers (chargebacks, disputes, regression detection)

### mcp-memory-service — persistent memory MCP
- **Repo**: `doobidoo/mcp-memory-service`
- **Does**: SQLite-vec (+ optional Neo4j) for vector memory; "Natural Memory Triggers" detect memory-relevant patterns mid-conversation
- **Why**: the maintainers are unusually honest about their claims (rare in this ecosystem)

### claude-mem — tiered AI-compressed summaries
- **Repo**: `thedotmack/claude-mem`
- **Does**: hooks-based session summarization with progressive disclosure on SessionStart
- **When**: you work across many short Claude Code sessions on the same project

### claude-code-router — cost routing proxy
- **Repo**: `musistudio/claude-code-router` (~26.4k stars)
- **Does**: tiktoken-aware proxy that routes requests between Anthropic and OpenRouter/DeepSeek/Ollama/Gemini based on context length, task type, or custom rules
- **Caveat**: introduces a single point of failure; recommended only after you've observed your workflow and know what policies you want to enforce
- **See**: `router-design.md` (deferred — after the observation period)

### BMAD-METHOD — spec-driven agentic workflow
- **Repo**: `bmad-code-org/BMAD-METHOD` (~43k stars, v6 current)
- **Does**: Scrum Master agent pre-compiles "story files" with full context embedded so Dev agent never re-reads the PRD
- **When**: spec-driven teams, feature-sized work; overkill for solo quick iteration

### Agent OS v3 — standards injection
- **Repo**: `buildermethods/agent-os`
- **Does**: lightweight standards via `index.yml` auto-detection
- **Why notable**: v3 release notes explicitly admit v1/v2 was redundant with Claude Code's native plan mode — rare intellectual honesty

## Tier 3 — popular but treat claims skeptically

### SuperClaude Framework
- **Repo**: `SuperClaude-Org/SuperClaude_Framework` (~22.3k stars)
- **Claims**: "70% token optimization"
- **Reality**: issue #286 shows this was computed from word count; real measured result is 33% reduction of memory-file tokens (roughly 4% of total context)
- **Still useful as**: curated scaffold of 18+ slash commands and personas

### Claude-Flow / Ruflo
- **Repo**: `ruvnet/ruflo` (~32k stars)
- **Claims**: 84.8% SWE-Bench, 0.95 truth-verification, multi-agent orchestration
- **Red flags**: Jan 2026 rebrand (trademark), complete v3 rewrite after one year, shifting tool counts (87 → 259 → 314), self-reported benchmarks without published methodology
- **Bottom line**: works but complexity-to-value ratio is high for most users

## Discovery — lists to follow

- `hesreallyhim/awesome-claude-code` — general Claude Code ecosystem
- `travisvn/awesome-claude-skills` — skills specifically; star-gated submission filters LLM-generated spam
- `anthropics/claude-cookbooks` — canonical reference; `context_engineering/` directory and `automatic-context-compaction.ipynb` especially
- Simon Willison's weblog — primary-source analysis
- `platform.claude.com/docs/en/release-notes/overview` — canonical changelog

## Install order for a new setup

1. `ccusage` + `ccstatusline` — 2 minutes, immediate value
2. Add `.claudeignore` to your repo — 5 minutes, biggest ROI fix
3. `ENABLE_TOOL_SEARCH=auto:5` if you run 3+ MCP servers — 1 minute
4. Serena MCP for any codebase work — 10 minutes including first-run indexing
5. OTel export if on API workloads — 30 minutes including dashboard setup
6. mcp-memory-service or claude-mem if cross-session state matters — 30 minutes
7. BMAD or Agent OS if you work spec-driven — afternoon investment, pays back over weeks
8. claude-code-router — **only after observing your workflow for 2–4 weeks**
