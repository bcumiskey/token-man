# Token-Man — Phase 2 Technical Specification

> Build brief for Claude Code (or any implementer) to ship Token-Man v1.0.

**Document version:** 1.0
**Target product version:** Token-Man v1.0 (first shipped build)
**Primary build tool:** Claude Code
**Status:** Complete. Ready to execute.

---

## 0. Read this first

This document is the single source of truth for what Token-Man v1.0 is and how to build it. It deliberately makes every technical decision so the implementer (likely Claude Code) doesn't have to guess. Where there are tradeoffs, the chosen path is stated and the alternatives are noted for future consideration.

**Inputs to this spec (already exist in the package):**
- `token-man-mockup.html` — visual reference for the HUD layout, styling, and interaction patterns
- `assets/token-man-full.svg` / `.emf` — full-body mascot for splash and icon bundle
- `assets/token-man-face.svg` / `.emf` — face-only mascot for title bar and tray icon
- `token-management/` — the companion skill, installable independently of the app

**Outputs of a successful build:**
- A signed Windows installer (`TokenMan-Setup-1.0.0.msi` or `.exe`) that installs an always-on-top desktop app
- A portable zip version of the same for no-install use
- An equivalent macOS `.dmg` and Linux `.AppImage` (cross-platform is cheap with Tauri; Windows is still the primary messaging)

---

## 1. Product overview

### 1.1 What Token-Man does

Token-Man is a desktop HUD (heads-up display) that unifies visibility into Claude token usage across every surface where a user interacts with Claude:

- **Claude Code** (all instances, including `claude-dispatch` sessions)
- **claude.ai** web/desktop/mobile chat (via Admin API aggregate)
- **Cowork** (via Admin API aggregate)
- **Claude for Chrome** (via Admin API aggregate)
- **Claude for Excel** (via Admin API aggregate)
- **Direct API workloads** (via Admin API)

It reads local files and calls public APIs. It never sends user data to third parties. It never calls Claude itself (except through the skill, which is a separate product).

### 1.2 What Token-Man explicitly does NOT do

- **Does not call Claude.** The HUD is a pure observer. The skill is the only place where Claude tokens get spent, and those are the user's normal conversational tokens.
- **Does not modify any Claude Code behavior.** It reads JSONL, OTel exports, and Admin API. It does not intercept, rewrite, or proxy requests.
- **Does not store conversation content.** Only metadata: session IDs, model names, token counts, cache hit rates, timestamps, project paths. Never message bodies.
- **Does not phone home.** No telemetry to the developer. No analytics. No update server other than the GitHub Releases feed for app updates (opt-in).

### 1.3 Core product loop

1. Background tailers watch ccusage JSONL files and a local OTel receiver.
2. A polling worker queries the Admin API on a configurable interval.
3. An aggregator computes the current state: per-source metrics, global totals, threshold status.
4. The renderer updates the webview HUD at a fixed cadence (default 500ms).
5. If any threshold is crossed, a notifier fires (sound, toast, red-mode styling).

Everything above happens inside a single Tauri process. There is no daemon, no separate service, no auto-start-by-default.

---

## 2. Stack

### 2.1 Chosen stack

- **Tauri 2** — desktop shell, window management, IPC, installer toolchain
- **Rust** — backend logic (tailers, API clients, aggregator, state store)
- **HTML / CSS / vanilla JS** — frontend rendered in the system webview; ported from `token-man-mockup.html`
- **SQLite** (via `rusqlite`) — local state store for rollups, alert history, profile config
- **TOML** — human-readable config files (profiles, thresholds)
- **`serde` / `serde_json`** — JSON handling for ccusage JSONL parsing
- **`reqwest`** — HTTP client for Admin API
- **`notify`** — filesystem watching for JSONL tailing
- **`rodio` or `cpal`** — audio playback for the jingle and alerts
- **`tauri-plugin-autostart`** — optional autostart at login (user-toggleable, off by default)
- **`tauri-plugin-window-state`** — remember window position/size between sessions

### 2.2 Why Tauri over Electron

Tauri 2 compiles to a ~10MB binary that uses the system webview (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux). Electron ships a full Chromium runtime (~150MB) for no functional benefit here. Since we're building a small always-on-top utility, size and memory footprint matter — Token-Man should be the kind of tool users forget is running.

### 2.3 Why vanilla JS over a framework

The frontend is a mockup HTML file with about 600 lines of CSS and 100 lines of vanilla JS driving animations. Adding React/Vue/Svelte would triple the bundle size to buy us nothing. Keep it vanilla. If the frontend grows past ~2000 lines or needs reactive state beyond what simple DOM manipulation handles, revisit.

### 2.4 Why SQLite over plain files

We need to:
- Query "how many tokens were used in the last 5 hours?" (window aggregates)
- Store rolling rollups for the spectrum display (60s / 1h / 24h)
- Persist alert history
- Survive app restarts without losing state

SQLite is the boring correct answer. Single file, zero setup, fully supported by Rust via `rusqlite`. Database lives at `%APPDATA%/Token-Man/state.db` on Windows (or the platform equivalent).

---

## 3. Architecture

### 3.1 Process layout

Token-Man is a single Tauri process with three internal logical modules:

```
┌─────────────────────────────────────────────────────────────┐
│                    Token-Man (single process)               │
├─────────────────────────────────────────────────────────────┤
│  Frontend (webview)                                         │
│    HUD render loop, user interactions, audio playback       │
│              ▲                           │                  │
│              │ IPC: state updates        │ IPC: commands    │
│              │                           ▼                  │
│  Backend (Rust)                                             │
│    ┌──────────────┬──────────────┬──────────────────────┐  │
│    │ Source       │ Aggregator   │ State store          │  │
│    │ workers      │              │ (SQLite + in-memory) │  │
│    │              │              │                       │  │
│    │ - ccusage    │ Computes     │ Rollups, alerts,      │  │
│    │   tailer     │ metrics,     │ config, profiles      │  │
│    │ - OTel       │ thresholds,  │                       │  │
│    │   receiver   │ derived      │                       │  │
│    │ - Admin API  │ state        │                       │  │
│    │   poller     │              │                       │  │
│    └──────────────┴──────────────┴──────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 Data flow

1. Each **source worker** runs on its own Tokio task. Events are pushed onto an `mpsc` channel.
2. The **aggregator** consumes from that channel, updates in-memory state, and periodically flushes rollups to SQLite.
3. A **render tick** (default 500ms, configurable 200–2000ms) reads current state and emits an IPC event to the webview.
4. The **webview** renders the state without doing any computation — it just reads the JSON and updates DOM.
5. **Commands from the UI** (e.g. "snap to clipboard", "trigger compact on session X") flow back to backend via IPC and are handled synchronously.

### 3.3 Render cadence and performance budget

- 500ms render tick = 2fps update rate. This is sufficient because most of the UI is slow-moving (context %, cost, ETA). The spectrum visualizer interpolates between ticks in CSS for visual smoothness.
- VU needle uses requestAnimationFrame for smoothness independent of the data tick.
- Target idle CPU usage: <0.5% on a modern machine. Target memory: <80MB resident (including webview).

### 3.4 Window behavior

- **Default window:** always-on-top, frameless (custom title bar drawn in HTML), 540×auto sized, positioned bottom-right on primary display first launch, draggable, resizable vertically only (width locked).
- **Minimize:** collapses to 420×28 strip. State transition is animated (120ms).
- **Close:** if "minimize to tray" is enabled in settings (default: on), closing the window hides it and puts a tray icon in the system notification area. Otherwise, close quits the app.
- **Tray icon:** face-only Token-Man logo. Single-click restores window. Right-click shows a menu: Show / Hide / Settings / Quit.
- **Pin toggle:** the 📌 button in the title bar toggles always-on-top. State is persisted.

---

## 4. Data sources (the hard part)

This is where most of the complexity lives. Each source has a different instrumentation model, a different data format, and a different failure mode. This section is exhaustive because getting this wrong breaks the whole product.

### 4.1 ccusage JSONL tailer (primary source)

**What it reads:** `%USERPROFILE%\.claude\projects\*\*.jsonl` (Windows) or `~/.claude/projects/**/*.jsonl` (macOS/Linux).

Claude Code writes one JSONL file per session, appending one line per turn. Each line includes fields like (verify these against current Claude Code schema at build time — they've changed before):

```json
{
  "uuid": "session-abc123",
  "timestamp": "2026-04-21T19:42:11.234Z",
  "type": "assistant",
  "message": {
    "model": "claude-sonnet-4-6",
    "usage": {
      "input_tokens": 1234,
      "output_tokens": 567,
      "cache_creation_input_tokens": 2048,
      "cache_read_input_tokens": 8192
    }
  },
  "cwd": "/Users/user/projects/my-project"
}
```

**Implementation:**
- Use `notify` crate to watch the `projects/` directory recursively for file changes.
- For each changed file, read only the new lines appended since last known offset.
- Parse each line as JSON. If parsing fails, log and skip (don't crash the tailer).
- Emit a `CCUsageEvent` onto the aggregator channel.
- Maintain per-file offset state in memory; flush to SQLite every 60s so we can resume from the right place on restart.

**Session identification:** the JSONL filename IS the session UUID. The filepath contains the project path. The file's mtime plus line count gives us "sessions with activity in the last N minutes" for the LIVE count.

**Session naming:** for display in the UI, extract the project name from the `cwd` field. If that's missing, fall back to the containing folder name.

**Dispatch detection:** if the environment variable or a specific field in the JSONL indicates this is a `claude-dispatch` session (verify at build), mark the source as `dispatch` rather than `code`.

**Failure mode:** if the JSONL directory doesn't exist (user hasn't used Claude Code yet), the tailer starts in a "waiting" state and polls for existence every 30s. It does NOT error out.

### 4.2 OTel receiver (secondary source for Claude Code)

**What it does:** Claude Code can be configured to export OpenTelemetry traces and metrics via the `CLAUDE_CODE_ENABLE_TELEMETRY=1` environment variable. Token-Man runs a local OTLP receiver on `127.0.0.1:<port>` to capture these.

**Why bother when we already have JSONL?** OTel gives us lower-latency updates (real-time streaming vs. file append polling) and richer metrics (tool call timings, sub-agent activity, model fallback events). For v1.0, the JSONL tailer is the primary source; OTel is a nice-to-have that improves latency and adds detail.

**Implementation:**
- Use the `opentelemetry-otlp` crate server side, or implement a minimal gRPC/HTTP receiver.
- Default port: 4317 (gRPC) or 4318 (HTTP). User-configurable in settings.
- If port is in use at startup, try the next available up to 4327 and log the chosen port. Show it in settings so users can configure `CLAUDE_CODE_TELEMETRY_ENDPOINT` to match.
- Capture metrics and forward to aggregator; cross-reference with JSONL data by session ID.

**Failure mode:** if no OTel traffic arrives within 60s of startup, mark as "not configured" in the sources list. Never treat absence as error.

### 4.3 Admin API poller

**What it does:** polls the Anthropic Admin API to fetch organization-level usage data that is NOT visible in Claude Code JSONL — specifically, usage from claude.ai, Cowork, Chrome, Excel, and direct API workloads outside Claude Code.

**Required credentials:** an Admin API key with appropriate scopes. Stored in OS keychain (Windows Credential Manager / macOS Keychain / Secret Service on Linux) via the `keyring` crate. Never written to disk in plaintext.

**Poll interval:** default 60s. Configurable 30s–600s. More frequent polling costs API quota.

**Endpoint reference:** the current Admin API usage endpoint as of build time (consult https://docs.claude.com for the current path — this has moved before). Typically something like `GET /v1/organizations/{org_id}/usage_report`.

**Response handling:** Admin API returns aggregate usage over a time window. Token-Man stores the most recent snapshot and displays it as the per-source values for the opaque sources (Chrome, Cowork, Excel). These sources are visually marked with the hollow/outlined dot in the UI to make clear that the numbers are aggregate-only, not real-time.

**Profile support:** each configured profile has its own Admin API key. Profiles can be filtered down by user (if the Admin API supports per-user scoping) or left aggregate.

**Failure mode:** if the Admin API call fails (network, auth, rate limit), show the last-known values with a "stale" indicator. Surface the error in the status bar: `REG — · ADMIN OFFLINE`. Auto-retry with exponential backoff starting at 60s, capped at 30min.

### 4.4 Source registry

The **model registry** (shown in the status bar as `REG 4.X · APR 16`) is a small JSON file bundled with the app that maps model names to their context windows, tokenizer inflation factors, pricing, and cache behavior. It lives at `assets/model-registry.json` inside the installed app.

Updates are checked on user click of the registry status cell. The check fetches a `model-registry.json` from the app's GitHub Releases page or a static URL. If a newer version exists, it's downloaded and swapped in atomically. This is an opt-in action, never automatic.

**Format:**
```json
{
  "version": "4.7",
  "updated": "2026-04-16T00:00:00Z",
  "models": {
    "claude-opus-4-7": {
      "context_window": 1048576,
      "tokenizer_inflation": 1.22,
      "pricing_input_per_mtok": 15.00,
      "pricing_output_per_mtok": 75.00,
      "cache_read_multiplier": 0.1,
      "cache_write_multiplier": 1.25
    }
  }
}
```

---

## 5. State model

### 5.1 Core data structures

```rust
// Simplified for spec; actual fields may include more.

pub struct AppState {
    pub sources: HashMap<SourceId, SourceState>,
    pub profiles: Vec<Profile>,
    pub active_profile: ProfileId,
    pub global_metrics: GlobalMetrics,
    pub alert_history: VecDeque<Alert>,  // last N alerts, ring buffer
    pub settings: Settings,
}

pub struct SourceState {
    pub id: SourceId,         // "code_1", "dispatch_2", "chrome", etc.
    pub kind: SourceKind,     // Code, Dispatch, Chrome, Cowork, Excel, Api
    pub owner: ProfileId,     // which profile this belongs to
    pub model: String,
    pub project: Option<String>,
    pub context_percent: Option<f32>,     // None for opaque sources
    pub tokens_per_min: Option<f32>,
    pub cost_today: f32,
    pub cache_hit_rate: Option<f32>,
    pub session_start: Option<DateTime<Utc>>,
    pub last_activity: DateTime<Utc>,
    pub is_opaque: bool,      // true for Chrome/Cowork/Excel
    pub status: SourceStatus, // Active, Idle, Stale, Offline
}

pub struct GlobalMetrics {
    pub tokens_per_min_current: f32,
    pub tokens_per_sec_vu: f32,  // for VU needle; short-window average
    pub live_sessions: u32,
    pub cost_today: f32,
    pub limit_eta: Option<Duration>,
    pub burn_rate_per_hour: f32,
    pub cache_hit_rate_global: f32,
    pub context_percent_worst: f32,    // the highest-context session
    pub five_hour_usage_percent: f32,
    pub cache_usage_percent: f32,
    pub week_usage_percent: f32,
    pub spectrum_60s: RingBuffer<(f32, f32)>,  // (in, out) tokens per tick
    pub spectrum_1h: RingBuffer<(f32, f32)>,
    pub spectrum_24h: RingBuffer<(f32, f32)>,
}

pub struct Alert {
    pub id: Uuid,
    pub kind: AlertKind,       // ContextHigh, CacheLow, BurnHigh, LimitEta, etc.
    pub source_id: Option<SourceId>,
    pub timestamp: DateTime<Utc>,
    pub value: f32,
    pub threshold: f32,
    pub resolved: bool,
    pub resolved_at: Option<DateTime<Utc>>,
}

pub struct Profile {
    pub id: ProfileId,
    pub name: String,          // "you", "alex", "team"
    pub color: String,         // hex, for the swatch
    pub admin_api_key_ref: Option<String>,  // keychain reference, never the key itself
    pub plan_type: PlanType,   // Pro, Team, Enterprise, Api
    pub five_hour_limit: u64,  // tokens
    pub weekly_limit: Option<u64>,
}

pub struct Settings {
    pub thresholds: Thresholds,
    pub render_interval_ms: u32,
    pub admin_poll_interval_s: u32,
    pub minimize_to_tray: bool,
    pub autostart_enabled: bool,
    pub notification_sound: bool,
    pub notification_toasts: bool,
    pub always_on_top_default: bool,
    pub red_mode_enabled: bool,
    pub ccusage_paths: Vec<PathBuf>,   // custom paths if non-default
    pub otel_port: u16,
    pub jingle_on_launch: bool,         // default: true
}

pub struct Thresholds {
    pub ctx_warn: f32,     // default 70.0
    pub ctx_critical: f32, // default 85.0
    pub cache_warn: f32,   // default 80.0 (below this)
    pub cache_critical: f32, // default 70.0 (below this)
    pub burn_warn: f32,    // default 3.00 ($/hr)
    pub burn_critical: f32, // default 5.00
    pub eta_warn_min: u32, // default 120 (2h)
    pub eta_critical_min: u32, // default 30
    pub five_hour_warn: f32,  // default 70.0
    pub five_hour_critical: f32, // default 85.0
}
```

### 5.2 State transitions

Token-Man has three global states that affect rendering:

- **OK** — no thresholds crossed. Default phosphor-green styling.
- **WARN** — at least one warning threshold crossed. Amber accents on the affected meters.
- **ALERT** (red mode) — at least one critical threshold crossed. Red pulse, red-mode border glow, scrolling title bar marquee, optional sound + toast.

Per-source states (Active, Idle, Stale, Offline) are independent of the global state. A source is:
- **Active** — new events in last 60s
- **Idle** — no events for 60s to 10min (still alive)
- **Stale** — no events for 10min+ (collapsed into history)
- **Offline** — source worker reports error state

### 5.3 Persistence

The SQLite database at `%APPDATA%/Token-Man/state.db` has these tables:

```sql
-- Raw event stream (last 7 days, then rolled up and deleted)
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,  -- epoch ms
    event_type TEXT NOT NULL,    -- "token_usage", "session_start", "session_end"
    model TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    cache_read_tokens INTEGER,
    cache_write_tokens INTEGER,
    cost_usd REAL,
    metadata TEXT  -- JSON blob for extras
);
CREATE INDEX idx_events_timestamp ON events(timestamp);
CREATE INDEX idx_events_source ON events(source_id);

-- Hourly rollups (kept forever)
CREATE TABLE hourly_rollups (
    hour_start INTEGER NOT NULL,  -- epoch ms, aligned to hour
    source_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    cache_write_tokens INTEGER NOT NULL,
    cost_usd REAL NOT NULL,
    PRIMARY KEY (hour_start, source_id, profile_id, model)
);

-- Alert history (last 1000 alerts kept)
CREATE TABLE alerts (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    source_id TEXT,
    timestamp INTEGER NOT NULL,
    value REAL,
    threshold REAL,
    resolved INTEGER NOT NULL DEFAULT 0,
    resolved_at INTEGER,
    metadata TEXT
);

-- File tailer state (resume points)
CREATE TABLE tailer_offsets (
    file_path TEXT PRIMARY KEY,
    byte_offset INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
);
```

---

## 6. Configuration

### 6.1 Config file location

`%APPDATA%/Token-Man/config.toml` (Windows), `~/Library/Application Support/Token-Man/config.toml` (macOS), `~/.config/token-man/config.toml` (Linux).

### 6.2 Example config

```toml
# Token-Man config
# This file is safe to edit by hand. App will reload on change.

[app]
render_interval_ms = 500
admin_poll_interval_s = 60
minimize_to_tray = true
autostart = false
notification_sound = true
notification_toasts = true
always_on_top_default = true
red_mode_enabled = true
jingle_on_launch = true
otel_port = 4318

[app.window]
# Last window position persisted here, edited by app at runtime.
x = 1200
y = 800
width = 540
height = 180

[thresholds]
ctx_warn = 70.0
ctx_critical = 85.0
cache_warn = 80.0
cache_critical = 70.0
burn_warn = 3.00
burn_critical = 5.00
eta_warn_min = 120
eta_critical_min = 30
five_hour_warn = 70.0
five_hour_critical = 85.0

[[profiles]]
id = "you"
name = "you"
color = "#378ADD"
admin_api_key_ref = "tokenman-profile-you"  # keychain entry name
plan_type = "Max"
five_hour_limit = 200000
weekly_limit = 5000000

[[profiles]]
id = "alex"
name = "alex"
color = "#993C1D"
admin_api_key_ref = "tokenman-profile-alex"
plan_type = "Pro"
five_hour_limit = 50000

# Optional: override default ccusage paths
# [sources]
# ccusage_paths = ["C:\\Users\\you\\.claude\\projects"]
```

### 6.3 Settings UI

A modal opened by the ⚙ button in the title bar. Tabs:

1. **General** — render cadence, startup behavior, minimize-to-tray, jingle toggle
2. **Profiles** — list, add, edit, delete; each profile has a name, color picker, API key field (stored in keychain), plan type dropdown, custom limits
3. **Thresholds** — sliders for all warn/critical levels; "reset to defaults" button
4. **Notifications** — sound toggle, toast toggle, red-mode styling toggle, test buttons
5. **Sources** — ccusage path config (auto-detect with override), OTel port, Admin API endpoint override
6. **About** — version, license, "check for updates" button, link to GitHub

---

## 7. Frontend specifics

### 7.1 Porting the mockup

The mockup HTML at `token-man-mockup.html` is the starting point. For v1.0:

1. Strip the `notes` section at the bottom (mockup-only explainer).
2. Strip the jingle hint (production plays the jingle on launch without needing a click, assuming `jingle_on_launch = true`).
3. Replace all placeholder values (`0,000`, `—%`, "no active session") with live data via IPC.
4. Wire up all buttons to send IPC commands: pin toggle, settings, minimize, close, snap, time-window tabs, registry refresh, session row clicks.
5. Add the splash screen as an initial overlay that fades out after 1.5s once the first data tick has arrived.

### 7.2 IPC interface

From backend to frontend — single event type, `state-update`, emitted on each render tick with the full renderable state:

```typescript
interface StateUpdate {
  globalState: 'ok' | 'warn' | 'alert';
  metrics: {
    tokensPerMin: number;
    tokensPerSec: number;
    liveSessions: number;
    costToday: number;
    limitEta: string | null;  // pre-formatted, e.g. "2h 40m"
    burnPerHour: number;
    cacheHitRate: number;
    ctxWorst: number;
    fiveHour: number;
    cacheUsage: number;
    week: number;
  };
  sources: SourceView[];       // array matching the session list
  spectrum: {
    window: '60s' | '1h' | '24h';
    in: number[];              // normalized 0-1 values for bars
    out: number[];
  };
  alerts: AlertView[];         // recent alerts for the status bar
  profile: { id: string; name: string; color: string };
  registry: { version: string; updatedAt: string };
  pinActive: boolean;
}
```

From frontend to backend — named commands:

- `toggle-pin`
- `minimize`
- `close` (may actually hide if minimize-to-tray is on)
- `snap-to-clipboard` → returns markdown string, which frontend copies
- `set-spectrum-window` with `{ window: '60s' | '1h' | '24h' }`
- `select-session` with `{ sourceId: string }`  — triggers expand-in-place
- `session-action` with `{ sourceId, action: 'focus' | 'compact' | 'clear' }`
- `refresh-registry`
- `open-settings`
- `set-profile` with `{ profileId: string }`

### 7.3 Audio

Jingle sequence: "Token-Man — it really whips the token's ass!" followed by a donkey hee-haw. Sample format: 16-bit PCM WAV, 44.1kHz, mono. Bundle as `assets/jingle.wav`. Total duration should be ≤3 seconds.

**Commission this asset.** In the meantime, the mockup's synthesized approximation lives in the HTML; the Tauri build should drop that code entirely and play the WAV via `rodio`.

Alert sound: short (≤800ms) notification tone. A single descending two-tone works well. Bundle as `assets/alert.wav`. Commission or synthesize.

Both sounds respect the `notification_sound` setting.

### 7.4 Notifications

Windows toast notifications via Tauri's native notification API. Triggered on:
- ALERT state entered (not re-triggered while alert is sustained)
- ALERT state resolved
- Source goes Offline unexpectedly (not on normal idle)

Toast contents:
- **Title:** "Token-Man" + variant emoji (📌 for info, ⚠️ for warn, 🔴 for alert)
- **Body:** short description with source + metric + value, e.g. "Code #1 context at 94% — compact recommended"
- **Actions (where supported):** "Dismiss" and "Open HUD"

---

## 8. Token efficiency safeguards (IMPORTANT)

**The product's entire reason to exist is helping users reduce Claude token spend. The product itself MUST NOT contribute meaningfully to that spend.** These constraints are non-negotiable:

### 8.1 HUD costs zero Claude tokens

The HUD reads local files and calls the Admin API (which costs nothing beyond the normal Admin API rate limit, which is generous and not user-token-priced). The HUD does not and must not call the Claude Messages API at any point.

### 8.2 Skill invocation budgets

The `token-management` skill is the only place where Token-Man-related activity can consume Claude tokens, and only when the user explicitly invokes Claude in a way that matches the skill's trigger. Even then:

- **SKILL.md frontmatter budget:** ≤200 tokens (always in context when skill is installed)
- **SKILL.md body budget:** ≤2500 tokens (loaded only when triggered)
- **Reference file budget:** each reference file ≤3000 tokens; loaded only when SKILL.md instructs Claude to read that specific file for this specific question
- **Total budget per invocation:** ≤5500 tokens beyond the user's message. If a question somehow requires more, the skill should direct the user to the GitHub repo instead of stuffing more into context.

### 8.3 Trigger specificity audit

The skill's trigger description must be specific enough that it fires on genuine token-management problems and NOT on casual mentions of "cost" or "token" in unrelated contexts. Before shipping, the build process should:

1. Review the trigger description for ambiguity
2. Test against a corpus of 20+ off-topic messages that mention token-related words casually ("let me show my token of appreciation", "the cost of this project", etc.)
3. Confirm the skill does not fire for those cases

This is a one-time test, not a runtime check.

### 8.4 Reference file gating

SKILL.md must include explicit, not-implicit, instructions about when to load each reference file. Example:

> Load `references/model-selection.md` ONLY when the user asks about choosing a model (Opus vs. Sonnet vs. Haiku) or reports a specific model-related problem. Do NOT load it for general token questions.

Not:

> ~~The references folder contains more info; consult as needed.~~

---

## 9. Build and distribution

### 9.1 Build targets

- **Windows:** MSI installer (signed with EV cert or similar), plus portable ZIP
- **macOS:** universal binary (x86_64 + arm64), DMG installer (signed + notarized)
- **Linux:** AppImage (unsigned, portable)

### 9.2 Code signing

- Windows: EV or OV code signing cert. Without one, users will see SmartScreen warnings on first run.
- macOS: Apple Developer ID certificate + notarization.
- Linux: no signing; users are used to unsigned AppImages.

Budget for certs if not already available; EV cert ~$300/year.

### 9.3 Update channel

GitHub Releases is the canonical distribution point. The app checks for updates on launch (if user opted in) and on-demand via Settings → About → Check for Updates. Updates are downloaded in-app but require user confirmation to install.

### 9.4 Release notes convention

Written in Token-Man's voice per the show bible. Example:

> **v1.0.3 — Token-Man learn about daylight saving**
> - Fix bug where 5-hour window showed wrong time twice a year. Token-Man did not understand the springing forward. Now understand. Mostly.
> - Cache hit rate display no longer shows "NaN%" when no data. Token-Man embarrassed by that one.
> - Small speed improvements. Token-Man run faster now. Not fast like cheetah. Fast like Token-Man.

---

## 10. Scope decisions

### 10.1 In scope for v1.0

- Windows primary, macOS + Linux included at no extra cost because Tauri is cross-platform
- ccusage JSONL tailing + OTel receiver + Admin API polling (all three sources)
- Multi-profile support
- Full settings UI
- System tray integration
- Audio jingle and alerts
- Red mode with visual + audio + toast notifications
- Snapshot export to clipboard as Markdown
- Session row expand-in-place with focus / compact / clear actions
- Auto-detect ccusage paths with manual override
- Model registry with manual-check updates

### 10.2 Explicitly out of scope for v1.0

- **Mobile companion apps** (iOS/Android read-only views). These require the desktop to push state to a cloud endpoint. Defer to v2.0.
- **Cloud sync between machines.** Single-machine only for v1.0.
- **Claude Code request interception / proxy.** The `claude-code-router` design sketch in the skill package is deferred.
- **Historical analytics UI.** v1.0 shows live state and recent alerts. Deep historical views defer to v1.1+.
- **Cost allocation / chargeback reports.** Team billing features defer to v2.0.
- **Plugin system.** No user-extensibility in v1.0.

### 10.3 Known risks and mitigations

- **JSONL schema changes:** Claude Code has changed its JSONL format between minor versions before. The parser must fail soft: skip lines it doesn't understand, log a warning, keep running. A `schema_version` field would help but may not exist. At build, verify against the then-current Claude Code version and document the assumed schema in code comments.

- **Admin API changes:** the Admin API's endpoint paths and response format have shifted. Build against the current docs. If the API returns unexpected data, log it, show "stale" in the UI, don't crash.

- **Token efficiency regression:** someone in a future release could accidentally make the skill invoke on too many queries. Mitigation: the skill audit in §8.3 should be re-run on every skill update.

- **Audio asset quality:** the jingle as synthesized is a placeholder. Before shipping v1.0, commission a proper recording. Budget ~$50–100 on Fiverr for voice + donkey sample sourced from freesound.org (CC0 or CC-BY).

- **Icon asset formats:** EMF is for Windows icon bundles. For cross-platform, generate PNG renders at 16/32/48/64/128/256 from the SVG at build time using `librsvg` or similar. Tauri's icon bundler handles this if fed the correct source files.

---

## 11. Implementation order (recommended)

A suggested sequence for Claude Code to tackle this:

1. **Scaffold** — `cargo tauri init`, get the default app running with the mockup HTML as the frontend.
2. **Window behavior** — implement pin toggle, minimize to strip, close-to-tray, position persistence.
3. **ccusage JSONL tailer** — the single biggest value add. Read-only first, plumb events through to the webview so the spectrum and session list show real data from Claude Code.
4. **State store + rollups** — SQLite schema, hourly rollup job, 60s/1h/24h ring buffers.
5. **Threshold engine + red mode** — watch state, fire alerts, style accordingly.
6. **OTel receiver** — lower priority but straightforward; parallelizable.
7. **Admin API poller** — bring in the opaque sources.
8. **Settings UI** — profile management, thresholds, preferences.
9. **Audio** — jingle, alert sounds, notification integration.
10. **Snapshot export, session actions** — the small quality-of-life features.
11. **Packaging + signing** — installer, notarization, GitHub Releases.

Total rough estimate: 2–4 weeks of focused Claude Code effort for a solo build. Can be compressed or expanded significantly depending on how much is parallelized and how polished the settings UI needs to be.

---

## 12. Success criteria for v1.0

The v1.0 build ships when:

- [ ] Installer works on a clean Windows 11 machine. User installs, runs, sees splash + jingle.
- [ ] Token-Man auto-detects ccusage JSONL location and begins tailing within 5 seconds of launch.
- [ ] Running a Claude Code session in another terminal window causes Token-Man's spectrum and session list to show live data within 2 seconds of token events.
- [ ] With a valid Admin API key configured, opaque sources (Chrome, Cowork, Excel) show aggregate usage from the API.
- [ ] Red mode triggers correctly when a manually-forced threshold is crossed (test: edit the config to set `ctx_critical = 10.0`, run a short Claude Code session, verify alert fires).
- [ ] Pin toggle works. Minimize-to-strip works and restores. Close-to-tray works.
- [ ] Snap-to-clipboard produces a valid Markdown snapshot.
- [ ] Settings UI can add/edit/remove profiles and change thresholds; changes persist across restarts.
- [ ] App idles at <80MB memory and <0.5% CPU on a mainstream machine.
- [ ] Installer is signed and doesn't trigger SmartScreen warnings.
- [ ] GitHub repo has a README, license, and link to releases.

When all of the above pass, Token-Man v1.0 ships.

---

## 13. After v1.0

Once the build is live and real users are running it, reassess in this order:

- **Telemetry (opt-in only, aggregate, privacy-preserving):** are the default thresholds right? What fraction of users hit red mode? Which sources matter most?
- **Mobile companion:** read-only iOS/Android app that mirrors the desktop.
- **Cloud sync:** same-user multi-machine continuity.
- **Claude Code router:** the deferred proxy design from the skill package. Revisit only if a real user need surfaces.
- **Team features:** billing allocation, team-wide dashboards, cost chargeback.
- **Token-Man voice lines:** event-driven voice snippets ("Bad token. BAD token."). Initially ship a small set; grow organically.

---

*Filed by design collaboration, April 2026. Ready for execution.*

*Token-Man whip the token's ass. Now you build the whip.*
