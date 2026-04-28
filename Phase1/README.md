# Token-Man + token-management skill

> **Token-Man.** It really whips the token's ass.

A two-part toolkit for managing Claude token usage across the full ecosystem (Claude Code, claude.ai, Cowork, Chrome, Excel, and direct API workloads).

## Package contents

- `token-management/` — an installable Claude skill (production-ready)
- `token-man-mockup.html` — Winamp-inspired HUD design mockup (visual reference)
- `assets/` — logo files in SVG (cross-platform) and EMF (Windows-native) formats
- `README.md` — this document

## 1. `token-management/` — a Claude skill

A progressive-disclosure skill that Claude invokes in-chat whenever you mention tokens, context, caching, cost, compaction, `/compact`, `/clear`, `/context`, `/cost`, ccusage, rate limits, or any of the symptoms those features address. Grounded in a four-layer model (measurement, budget, optimization, governance) with reference files covering model selection, compaction playbook, cache strategy, per-surface instrumentation, and vetted community tools.

**Install:** drop the `token-management/` folder into `~/.claude/skills/` for user-scope availability across all projects, or into `.claude/skills/` in a specific repo for project-scope. Restart the Claude Code session, run `/skills` to confirm it's listed. From there, any message matching the skill's trigger description invokes it — no manual activation needed.

**Scope:** works on any Claude account. Nothing in the skill is personalized, account-bound, or requires an API key.

**Included files:**
- `SKILL.md` — the always-in-context frontmatter and the triage workflow
- `references/model-selection.md` — Opus/Sonnet/Haiku decision tree with the Opus 4.7 tokenizer inflation note
- `references/compaction-playbook.md` — decision tree for `/clear` vs `/compact` vs subagent handoff vs memory tool
- `references/cache-strategy.md` — prompt-caching placement, invalidators, TTL tradeoffs, Batch API stacking
- `references/surface-map.md` — per-surface instrumentation inventory and environment variables
- `references/community-tools.md` — honest tiering of ccusage, ccstatusline, Serena, mcp-memory-service, claude-code-router, and others
- `router-design.md` — deferred design sketch for `claude-code-router` to revisit after 2–4 weeks of observation

## 2. `token-man-mockup.html` — HUD design mockup

A standalone HTML file previewing the planned always-on-top token HUD. Open it in any modern browser (Chrome, Firefox, Edge, Safari) — no install, no dependencies. References the logo files in `assets/`, so keep the folder together when moving the file.

**What it shows:**
- A launch splash screen featuring Token-Man in full warrior stance
- The main HUD layout with phosphor-green LCD readouts, analog VU needle, dual-channel spectrum with peak-hold and budget overlay, per-source session list, and red-alert mode styling
- The minimized-strip variant (28px tall)

**The jingle:** click anywhere on the page to hear the intro. Browsers block autoplay audio until user interaction, so the first click triggers "Token-Man — it really whips the token's ass" followed by a donkey hee-haw. The production Tauri build plays this on launch without needing a click. Yes, it's the old WinAmp joke.

**What it doesn't do:** no real data ingestion, no always-on-top behavior, no clipboard export, no actual integration. This is the reference design, not the app.

## 3. `assets/` — logo files

- `token-man-full.svg` — full-body Token-Man mid-swing, cross-platform vector format, used by the mockup splash screen
- `token-man-face.svg` — face-only Tiki variant, used in the title bar at 14px
- `token-man-full.emf` — full-body, Windows Enhanced Metafile format (native Windows vector, for the production Tauri app's Windows icon bundle)
- `token-man-face.emf` — face-only, EMF format

SVGs render on any platform. EMFs are the correct format for Windows tray icons, taskbar badges, and system notifications when the production app ships.

## Building the real thing

The production HUD is a Tauri 2 desktop application (Rust backend + webview frontend) that reads ccusage JSONL files from `%USERPROFILE%\.claude\projects\`, consumes Claude Code OTel export, and polls the Admin API for organizational usage data. Use this mockup plus a forthcoming Phase 2 specification document as inputs to Claude Code to scaffold the actual app.

## Why this exists

Anthropic ships a lot. Release velocity has made token management genuinely hard — not because the individual features are bad but because they're fragmented across surfaces with wildly different instrumentation. Claude Code exposes `/context`, `/cost`, `/compact`, hooks, and OTel export; claude.ai, Cowork, Chrome, and Excel expose almost nothing. You can't manage what you can't see, and nobody ships a unified view.

Token-Man addresses the gap at two layers. The skill gives you a diagnostic partner inside Claude that knows the current landscape and intervenes at the right moment. The HUD gives you ambient visibility — a small always-on-top panel showing aggregate state across every source you have, with honest indicators where data is opaque.

They're designed to work together but each stands alone.

## Status

- **Skill:** production-ready, install and use.
- **HUD:** design stage — mockup only. Real build is the next step.
- **Phase 2 spec:** pending.

## License and attribution

Free to share, fork, and modify. Built collaboratively using Claude. If you extend or improve either piece, a pointer back is appreciated but not required.
