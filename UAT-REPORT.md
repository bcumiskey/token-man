# Token-Man v1.0 — Pre-Release UAT Audit

Static code audit performed against `Phase2/PHASE-2-SPEC.md`. No runtime execution. No source files modified.

---

## Findings

### [High] Cost formula ignores the spec's dedicated cache-write price column

**Category:** A
**Location:** `app/src-tauri/src/registry.rs:45-47`
**Observed:** `write_cost` is computed as `pricing_input_per_mtok * cache_write_multiplier` (1.25× input). The registry JSON schema does not define a separate cache-write price field — only `cache_write_multiplier` scaling `pricing_input_per_mtok`. Cache reads similarly use `pricing_input_per_mtok * cache_read_multiplier` (0.1×).
**Expected:** Per spec §4.4 the registry is the authority on "cache behavior". The current math is defensible but conflates the multiplier semantics — the spec shows `cache_read_multiplier: 0.1` and `cache_write_multiplier: 1.25`. Applying those against the *input* price matches Anthropic's actual cache pricing, so the formula is correct; however, the `tokenizer_inflation` field is parsed and never applied anywhere, which is the real bug — ccusage tokens are the "displayed" (not true) tokens and the registry supplies the inflation factor to correct this.
**Reproduction:** n/a — static analysis (grep `tokenizer_inflation` — never read).
**Recommended fix:** Either drop `tokenizer_inflation` from the registry schema or apply it when comparing to context window / computing %ctx.

---

### [High] Context % is computed from a per-turn delta, not running total

**Category:** A
**Location:** `app/src-tauri/src/sources/ccusage.rs:248-252` and `aggregator.rs:144-147`
**Observed:** `context_tokens` is set to `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` **from that single assistant turn**. `aggregator.handle_usage` then computes `context_percent = ctx_tokens / window * 100`. ccusage's per-turn `usage.input_tokens` includes the cumulative prompt on each turn *for pre-4.x Claude Code*, but cache_read/cache_write are deltas. Summing them over-counts on early turns and under-counts on tool-heavy turns.
**Expected:** Context % should reflect the current prompt size (cumulative cache + live input), not a per-turn sum. Per spec §4.1 the tailer must "verify against current Claude Code schema at build time".
**Reproduction:** n/a — static analysis.
**Recommended fix:** Track the max of `input_tokens + cache_read + cache_write` observed in the session (latest turn reflects full prompt), not sum; revisit against current Claude Code JSONL schema.

---

### [High] Burn rate calculation is an approximation, not time-based

**Category:** A
**Location:** `app/src-tauri/src/aggregator.rs:275-294`
**Observed:** Burn rate is computed by scaling `cost_today * (recent_tokens / today_tokens)` and multiplying by 6. The `_i, _o` DB query result is discarded. This is labeled "approximation" in a comment. After app runs for many hours, `today_tokens` grows huge while `recent_tokens` is over 10 min, so the ratio gets very small; the 6× factor (10 min → 1 hour) only approximates when cost-per-token is stable.
**Expected:** Burn rate should be `Σ(cost over last 10 min) * 6`. The DB already stores per-event `cost_usd`; a `SELECT SUM(cost_usd) FROM events WHERE timestamp >= ?` would be exact.
**Recommended fix:** Add a `sum_cost_since` DB helper and use it.

---

### [High] Week usage limit hard-coded in aggregator when profile lacks weekly_limit

**Category:** A / F
**Location:** `app/src-tauri/src/aggregator.rs:179-181`
**Observed:** When active profile has `weekly_limit = None`, the aggregator falls back to `5_000_000`. The 5-hour limit similarly falls back to `200_000`. These magic numbers are not in the spec's defaults list and don't match the `Profile` default that already sets `five_hour_limit = 200_000, weekly_limit = Some(5_000_000)` — so the fallback should never fire for the shipped default profile, but profiles added via UI default `weekly_limit: null` and `five_hour_limit: 50000` (see `settings.js:91`), meaning new profiles will silently use a 200k fallback instead of the 50k configured limit.
**Expected:** Respect the explicit `five_hour_limit` on the profile; treat `weekly_limit = None` as "no weekly display" (render "—").
**Recommended fix:** Remove the `.unwrap_or(200_000)` on `five_hour_limit` (always Some in config). Show week % only when `weekly_limit.is_some()`.

---

### [High] Stale-source detection never evicts `live_sessions` count when source is Idle/Stale

**Category:** A
**Location:** `app/src-tauri/src/aggregator.rs:212-219`
**Observed:** Status decay sets Stale first, then Idle, then "else Active". Sources that *became* Idle or Stale keep their current status but are only *counted* as live when re-promoted to Active. The logic is correct, but `live` is only incremented when status transitions to Active, not when already Active. Re-reading: the `else if !Offline { status = Active; live += 1 }` branch runs whenever last_activity >= live_cutoff, so Active count is correct. **On closer inspection this is fine** — withdrawing.

(Reclassified: not a bug.)

---

### [Medium] `cache_usage_percent` is set equal to `cache_hit_rate_global`

**Category:** A
**Location:** `app/src-tauri/src/aggregator.rs:272`
**Observed:** `s.global.cache_usage_percent = s.global.cache_hit_rate_global;` — the frontend meter labeled "cache" therefore shows hit rate (a percentage of cache reads vs input), not cache utilization. The HUD shows both `cacheHitRate` (eta row) and `cacheUsage` (meter rack), both identical values from a single source.
**Expected:** Spec §5.1 lists `cache_usage_percent` and `cache_hit_rate_global` as distinct fields in `GlobalMetrics`. Likely intent: one is read-hit ratio, the other is fraction-of-budget-cached or context-filled-with-cache.
**Recommended fix:** Define and implement a distinct semantic for `cache_usage_percent` or remove the duplicate field.

---

### [Medium] Cache-warn/critical threshold semantic is inverted but meter styling uses the wrong inversion

**Category:** A / C
**Location:** `app/src-tauri/src/thresholds.rs:48-57` vs `app/src/main.js:116-125, 147`
**Observed:** Backend uses `cache_warn = 80.0, cache_critical = 70.0` with `cmp: |v, thr| v <= thr && v > 0.0` — alerts fire when hit rate drops below the threshold. Good. But the frontend meter uses `classForPercent(pct, 70, 85, invert=true)` with hard-coded thresholds; inversion rule: `v <= 100 - crit` → crit. That's `v <= 15%` for crit and `v <= 30%` for warn, which doesn't match the 70/80 backend thresholds at all.
**Expected:** Frontend meter color should match backend cache thresholds (warn at ≤80%, crit at ≤70%).
**Recommended fix:** Pass thresholds from backend or pass them through StateUpdate so the frontend uses them literally; drop the inversion math.

---

### [Medium] Hysteresis counter is process-global, not per-AppState

**Category:** B
**Location:** `app/src-tauri/src/thresholds.rs:15-19, 142`
**Observed:** `clear_counters()` is a `static OnceLock<Mutex<HashMap<…>>>`. Uses `.unwrap()` on lock. State is never reset across profile changes or config reloads.
**Expected:** Per spec §5.2, states belong to `AppState`. Process-static is fine in a single-instance app, but `.lock().unwrap()` can panic on poisoned mutex.
**Recommended fix:** Move counters into `AppState`; or at least handle poisoning (`.lock().unwrap_or_else(|e| e.into_inner())`).

---

### [Medium] `compute_global_state` re-derives severity instead of storing it

**Category:** B
**Location:** `app/src-tauri/src/thresholds.rs:90-113`
**Observed:** Comment admits the function infers severity from `value / threshold` ratio: "10% past threshold" → Alert, otherwise Warn. This is fragile: an alert fired at `ctx_warn=70` against value=72 is Warn. If value later drifts to 78 it is still Warn (correct). But if `ctx_critical=85` and value rises to 80, the stored `threshold` is still 70 (warn tier), and 80/70=1.14 > 1.10 → globally classified as Alert even though the critical threshold (85) was never crossed. This yields false Alert states between warn and critical.
**Expected:** Store the tier (Warn/Alert) on the alert record itself and read it back.
**Recommended fix:** Add `severity: GlobalState` field to `Alert` struct; set when alert is fired or upgraded.

---

### [Medium] Alerts are never "upgraded" from Warn to Alert as value worsens

**Category:** B
**Location:** `app/src-tauri/src/thresholds.rs:136-165`
**Observed:** `pre_existing` check causes new alerts to be suppressed whenever an unresolved alert of the same kind exists, regardless of whether the crossed tier has changed from warn → critical. The `threshold` field on the alert record is not updated when severity escalates.
**Expected:** When value crosses from warn tier into critical tier, either update the stored `threshold` or fire a fresh alert so toast/sound triggers.
**Recommended fix:** Detect tier escalation explicitly and re-emit/notify.

---

### [Medium] `SourceOffline` alert kind is defined but never fired

**Category:** B / I
**Location:** `app/src-tauri/src/state.rs:69`; no producer anywhere
**Observed:** `AlertKind::SourceOffline` has rendering/notification wiring (`aggregator.rs:385-388`) but no call site creates one. Spec §7.4 requires "Source goes Offline unexpectedly" → toast.
**Recommended fix:** Wire admin poller / ccusage watcher failures to emit this alert.

---

### [Medium] `evaluate` never clears resolved alerts from the deque, ring buffer grows unbounded until cap hit

**Category:** B
**Location:** `app/src-tauri/src/thresholds.rs:159-163`
**Observed:** Ring cap of 128; only pops on push. `compute_global_state` walks all alerts each tick. For low-volume use this is fine; for a long-running session with many resolved alerts, `evaluate` iterates all 128 each tick. Minor perf concern, not a bug.

---

### [Medium] `active_profile` is rendered literally as both `id` and `name`, and color is hard-coded

**Category:** C / D
**Location:** `app/src-tauri/src/state.rs:311-315`
**Observed:** `ProfileView { id: active_profile, name: active_profile, color: "#378ADD" }` — the view always shows active profile id as the name and a single blue color regardless of the `Profile.color` field. Multi-profile support is silently broken for the title-bar swatch.
**Expected:** Spec §5.1 gives each profile a distinct color.
**Recommended fix:** Look up the `Profile` record for `active_profile` in `config.profiles` and populate color/name from it.

---

### [Medium] Minimize button toggles window height but does not animate and does not collapse to the spec's 420×28 strip

**Category:** C
**Location:** `app/src-tauri/src/ipc.rs:29-38`; `tauri.conf.json:18-20`
**Observed:** Toggles between 540×32 and 540×280. Spec §3.4 requires 420×28 with 120ms animation. Width locked to 540 in the conf; minWidth is 420. No animation.
**Recommended fix:** Match spec: 420 width for minimized, animate via CSS transition or Tauri window-manager tween.

---

### [Medium] `cmd_minimize` size-toggle uses a local `_full` and `target_h` without persisting minimize state

**Category:** C
**Location:** `app/src-tauri/src/ipc.rs:33-36`
**Observed:** Logic infers "full" from current height > 60 on every click. A user who manually resizes near this boundary can end up in an inconsistent toggle.
**Recommended fix:** Track minimized state explicitly on AppState or via a window userdata flag.

---

### [Medium] Pin toggle default mismatches config default

**Category:** C / F
**Location:** `tauri.conf.json:23` `"alwaysOnTop": false` vs `config.rs:44` `always_on_top_default: true`; `index.html:33` pin button has `class="wa-tbtn active"` hard-coded
**Observed:** The window is created with alwaysOnTop=false, but config says default should be true, and the pin button UI is rendered "active" on load. The actual pin state is only corrected once the first `state-update` arrives (which sets `pin_active` from config). Brief visual/actual mismatch on launch.
**Recommended fix:** Set `alwaysOnTop: true` in tauri.conf.json or call `set_always_on_top(config.app.always_on_top_default)` in setup before showing window.

---

### [Medium] Splash hidden timer (800ms/1300ms) differs from spec's 1500ms

**Category:** C / H
**Location:** `app/src/main.js:264-266`
**Observed:** `setTimeout(... 800)` starts fade; `setTimeout(... 1300)` removes. Spec §7.1 says "fades out after 1.5s once the first data tick has arrived".
**Recommended fix:** Bump to 1500ms total hold + fade.

---

### [Medium] LCD "TOK/MIN" formatter drops thousands separator for numbers <1000 and shows raw integer; display will never match "0,000" placeholder styling

**Category:** C
**Location:** `app/src/main.js:110-114`
**Observed:** Under 1,000, `fmtNum` returns `Math.round(n).toString()` with no comma. The HTML placeholder is `0,000`, suggesting fixed-width numeric display. A value of 42 renders as `42` (narrow), shifting adjacent cells.
**Recommended fix:** Use `toLocaleString()` for all sub-million numbers; consider padding with leading zeros to keep layout stable.

---

### [Medium] Spectrum uses a single fixed 40-slot ring for all three windows; 1h/24h downsampling is time-counter-driven, not wall-clock-driven

**Category:** A / C
**Location:** `app/src-tauri/src/aggregator.rs:346-359`
**Observed:** Every 90 ticks of a 1s interval → 1h bar. Every 2160 ticks → 24h bar. Counter starts at 0 and uses `% 90 == 0` — that fires at tick 0 (on startup with no data), then 90, 180, … which *happens* to approximate the correct cadence, but if the app is paused/tokio starves, ticks drift from wall clock. With 40 slots × 90s per slot = 3600s = 1h window. For 24h: 40 × 2160s = 86,400s = 24h. Math checks out, but the 24h history is only populated after 24h of uninterrupted run.
**Expected:** On restart, 1h/24h bars read from the SQLite `hourly_rollups` table (spec §5.3). The table is declared but never written or read.
**Recommended fix:** Implement hourly rollup job; seed spectrum_1h and spectrum_24h from DB on startup.

---

### [Medium] `hourly_rollups` table is declared but never populated or read

**Category:** A
**Location:** `app/src-tauri/src/db.rs:127-138`
**Observed:** Schema created, no `INSERT`/`SELECT` against it anywhere.
**Expected:** Spec §5.3 mandates hourly rollups "kept forever".
**Recommended fix:** Implement a periodic rollup flush in aggregator.

---

### [Medium] Session ID / source ID derivation doesn't include profile or differentiate `claude-dispatch`

**Category:** A / E
**Location:** `app/src-tauri/src/sources/ccusage.rs:188,239-240`
**Observed:** All ccusage sessions get `source_id = "code:<uuid>"` and `kind: SourceKind::Code`, `owner: "you"`. Dispatch sessions are not detected per spec §4.1 requirement. Owner is hard-coded "you" — incompatible with multi-profile routing.
**Recommended fix:** Detect dispatch via env var/field; derive owner from active_profile or project mapping.

---

### [Medium] Admin poller `status = Active` on AdminSnapshot regardless of staleness

**Category:** A / E
**Location:** `app/src-tauri/src/aggregator.rs:84-95`
**Observed:** Any AdminSnapshot marks the source Active + updates last_activity to now. Since opaque sources never produce idle transitions naturally (polled on 60s cadence, each poll resets Active), they will remain "active" forever in the HUD, inconsistent with visual dot semantics for opaque (hollow).
**Recommended fix:** Use `SourceStatus::Idle` or a distinct status for opaque snapshot polling; let the hollow dot carry the meaning.

---

### [Medium] OTel receiver never produces events (drops payloads)

**Category:** E
**Location:** `app/src-tauri/src/sources/otel.rs:63-68`
**Observed:** Accepts bodies, returns 200, never forwards anything. Spec §4.2 explicitly allows v1.0 to be presence-only, but the README and the aggregator have no "connected" signal surfaced in the UI either. Status bar never shows OTel connectivity. The field `_tx` is underscore-prefixed signaling intentional unuse.
**Expected:** At least emit a StatusChange to flip an "otel: connected" source state.
**Recommended fix:** Emit an event on first successful POST.

---

### [Medium] Admin poll sleep math can sleep less than one interval

**Category:** E
**Location:** `app/src-tauri/src/sources/admin.rs:74-79`
**Observed:** `sleep.max(backoff.min(interval))` — if backoff exceeds interval (post-failure), it gets clamped back to interval by `.min()`. Intent appears to be "at least `interval` but also honor backoff", which should be `backoff.max(interval)`, not `.min()`. Current code silently ignores exponential backoff.
**Recommended fix:** `tokio::time::sleep(backoff.max(sleep)).await;`.

---

### [Medium] Tailer offsets are only persisted in memory, never flushed to SQLite

**Category:** A / E
**Location:** `app/src-tauri/src/sources/ccusage.rs:71,214`; `db.rs:58-81`
**Observed:** `Db::get_tailer_offset` / `set_tailer_offset` exist but ccusage uses an in-memory `HashMap` and never calls them. Spec §4.1: "flush to SQLite every 60s so we can resume from the right place on restart." On restart, the tailer skips existing content entirely (line 83-85), which loses any data written while the app was down.
**Recommended fix:** On startup, read offsets from DB for existing files instead of defaulting to file size; flush offsets periodically.

---

### [Medium] ccusage tailer can miss the first turn of a brand-new session

**Category:** A
**Location:** `app/src-tauri/src/sources/ccusage.rs:82-86`
**Observed:** On startup, all existing jsonl files have their offset set to current file size — any data written during the app's downtime is permanently skipped even for active sessions that are still running.
**Recommended fix:** Seed offset to 0 for files with mtime within the last 5 minutes or rely on persisted offsets (see above).

---

### [Medium] Settings → Save closes window immediately; profile changes that fail keychain write are silently lost from the config

**Category:** D
**Location:** `app/src/settings.js:126-137`
**Observed:** `await invoke("cmd_set_admin_key", ...)` is called before `cmd_save_config`, but no try/catch. If keyring fails (e.g., Windows Credential Manager unavailable in a sandbox), the exception propagates, the config is never saved, and the window closes before showing any error. User thinks they saved.
**Recommended fix:** Try/catch around keychain writes; surface error to user.

---

### [Medium] `cmd_set_profile` does not persist to disk

**Category:** D / F
**Location:** `app/src-tauri/src/ipc.rs:143-154`
**Observed:** Writes to in-memory config only; comment acknowledges "path resolution isn't available without AppHandle; caller via save_config." But the frontend has no mechanism bound to re-save when `cmd_set_profile` is invoked. Result: profile change is lost on app restart.
**Recommended fix:** Take `AppHandle` param and save immediately, or require caller to subsequently invoke save.

---

### [Medium] Settings UI has no profile selector on the main HUD; `cmd_set_profile` is defined but never invoked by frontend

**Category:** D
**Location:** `app/src/main.js` / `index.html:22-26`
**Observed:** `profileBtn` exists in the title bar with a `▾` chevron implying a dropdown, but no click handler wires it to anything. `cmd_set_profile` is reachable only by hand-crafted IPC.
**Recommended fix:** Wire the profile pill to a dropdown calling `cmd_set_profile`.

---

### [Medium] Session-row click toggles local `expandedSession` state but the row is re-rendered on every 500ms tick, so the expanded state is preserved only because of the module-global variable

**Category:** C
**Location:** `app/src/main.js:108, 241-247`
**Observed:** `innerHTML = html` replaces rows on every tick; attaches click handlers fresh every tick. Works but thrashes DOM 2× per second unnecessarily and loses any focus state. The "expand-in-place" shows only cache % and status — spec §7.2 shows `session-action` focus/compact/clear, but per README those are deferred to v1.1.
**Recommended fix:** Diff sessions instead of full rewrite; document expanded-pane contents as intentional minimal.

---

### [Medium] Alerts strip color uses `a.value >= a.threshold * 1.1` with no consideration of inverse metrics

**Category:** B / C
**Location:** `app/src/main.js:192`
**Observed:** CacheLow/LimitEta use `v <= thr`; for those, `value >= threshold * 1.1` is always false for an active alert. Alerts of those kinds will never show red in the strip, only amber.
**Recommended fix:** Match the backend inversion logic in `thresholds.rs:103-106`.

---

### [Low] Hard-coded profile id "you" everywhere in ccusage + SessionStart paths

**Category:** A / D
**Location:** `aggregator.rs:71`, `sources/ccusage.rs:240`
**Observed:** `"you".into()` literal as owner/profile. If the single profile is renamed, sessions are attributed to a non-existent profile.

---

### [Low] Snap markdown uses `{:?}` debug format for state

**Category:** D
**Location:** `app/src-tauri/src/snapshot.rs:14`
**Observed:** Renders `State: **Ok**` / `**Warn**` / `**Alert**` via Debug. OK in passing but brittle if enum naming changes.

---

### [Low] `BackendCommand::SelectSession` is just logged; expand is frontend-only

**Category:** D
**Location:** `app/src-tauri/src/aggregator.rs:412-414`
**Observed:** `debug!("select session {id}");` and returns. Cmd has no backend effect. Harmless; consider removing the ipc round-trip or repurposing.

---

### [Low] `_kinds`, `_win`, `_gs`, `_ts` dead-code stubs at aggregator.rs:434-459

**Category:** J
**Location:** `app/src-tauri/src/aggregator.rs:433-459`
**Observed:** Four `#[allow(dead_code)]` functions labeled "Keep sink used to suppress unused warning during iteration" — leftover scaffolding. Remove for v1.0.

---

### [Low] Unused struct field `OtelState._tx`

**Category:** J
**Location:** `app/src-tauri/src/sources/otel.rs:26`
**Observed:** `_tx: mpsc::Sender<SourceEvent>` never read. Remove or wire.

---

### [Low] `RegistrySection` is parsed but the URL default is a repo that likely doesn't exist yet

**Category:** F
**Location:** `app/src-tauri/src/config.rs:125-127`
**Observed:** `https://raw.githubusercontent.com/tokenman/token-man/main/model-registry.json` — placeholder. Registry refresh will fail against a 404 silently with "REG FAIL" flash.

---

### [Low] Admin API endpoint default guesses a path ("/v1/organizations/usage")

**Category:** E
**Location:** `app/src-tauri/src/sources/admin.rs:32`
**Observed:** Hard-coded URL differs from the spec's example (`/v1/organizations/{org_id}/usage_report`). Spec §4.3 notes this API "has moved before" — flagged as expected, but the default will 404 on every user's first install.

---

### [Low] Spec requires `schema_version` consideration in JSONL parser; parser silently skips unknown lines

**Category:** A
**Location:** `app/src-tauri/src/sources/ccusage.rs:203-212`
**Observed:** Skips unparseable lines. No logging escalation after N consecutive failures — a schema change would silently reduce data without alerting the user.
**Recommended fix:** Emit an alert/status-bar banner after 10+ consecutive parse failures.

---

### [Low] Tauri config `alwaysOnTop: false` but README/spec say default pinned

**Category:** F / H
**Location:** `tauri.conf.json:23` (see also Pin default mismatch above)

---

### [Low] Window start position is not "bottom-right on primary display" per spec §3.4

**Category:** C
**Location:** `tauri.conf.json:12-27`
**Observed:** No position configured. `tauri-plugin-window-state` will remember position after first move, but on first launch the window appears centered (Tauri default).
**Recommended fix:** Compute bottom-right on first boot.

---

### [Low] `cmd_close` always hides when minimize_to_tray=true, even on app quit intent; no "really quit" affordance

**Category:** C / D
**Location:** `app/src-tauri/src/ipc.rs:41-51`
**Observed:** With the default `minimize_to_tray = true`, clicking × hides the window. Only way to quit is tray → Quit. Power users expect Shift+× or similar; no keyboard shortcut bound.

---

### [Low] Audio WAV assets are present but tiny

**Category:** H
**Location:** `app/src-tauri/assets/alert.wav` (26,504 bytes), `jingle.wav` (176,444 bytes)
**Observed:** Files exist (non-empty). README confirms they are "synthesized placeholders." Spec §7.3 says commission real audio before GA.
**Recommended fix:** Commission recordings per spec.

---

### [Low] Icons listed in `tauri.conf.json` include `.icns` (macOS) and `.ico` (Windows) but README says icons are placeholders

**Category:** H
**Location:** `tauri.conf.json:40-46`; `src-tauri/icons/` contains icon.ico, icon.icns, icon.png plus sizes.
**Observed:** Files present but README §"Known gaps" lists "Icons are placeholders, not the Token-Man mascot rendered at 16/32/48/128/256". 32x32 and 128x128 present; 16/48/256 also present (256x256.png was listed). Spec §10.3 references EMF sources; no EMF in icons dir.

---

### [Low] Tray icon inherits `default_window_icon`, not the face-only mascot

**Category:** C / H
**Location:** `app/src-tauri/src/tray.rs:19-21`
**Observed:** Spec §3.4 says "Tray icon: face-only Token-Man logo." Current code uses the window icon which is the generated-placeholder 32x32.
**Recommended fix:** Bundle a tray-specific face-only icon asset.

---

### [Low] No multi-instance guard

**Category:** I
**Location:** `tauri.conf.json`; no `tauri-plugin-single-instance` in Cargo.toml
**Observed:** Starting Token-Man twice spawns two processes tailing the same JSONL and double-emits events. Spec doesn't explicitly require single-instance, but double-counting breaks every metric.
**Recommended fix:** Add `tauri-plugin-single-instance`.

---

### [Low] Clock skew: JSONL timestamp used raw for recent_ticks cutoff with `Utc::now()`

**Category:** A / I
**Location:** `app/src-tauri/src/aggregator.rs:141, 155-163`
**Observed:** `src.last_activity = u.timestamp` (from JSONL), then pruning uses `Utc::now() - 60s`. If the JSONL timestamp is skewed (e.g., user's clock drifted or event is from a file written an hour ago on a crash recovery), events may be instantly pruned or never pruned.
**Recommended fix:** Use `Utc::now()` for `last_activity` and ring insertion time, not source timestamp.

---

### [Low] `config.toml` parse failure silently falls back to defaults without touching the bad file

**Category:** F / I
**Location:** `app/src-tauri/src/config.rs:156-163`
**Observed:** Uses defaults in-memory but leaves the corrupt file on disk. Next save overwrites silently. Warns via `tracing::warn` but user won't see it. Acceptable fallback, but UI has no indication config was invalid.

---

### [Low] `cmd_refresh_registry` success doesn't persist the fetched registry to disk

**Category:** D / F
**Location:** `app/src-tauri/src/ipc.rs:96-115`
**Observed:** Updates in-memory `state.registry` only. On next launch, bundled `model-registry.json` is re-loaded; the fetch must be re-triggered.
**Recommended fix:** Write to `app_data_dir/model-registry.json` and prefer that on load.

---

### [Low] `tokenizer_inflation` field parsed but never applied

**Category:** A / J
**Location:** `app/src-tauri/src/registry.rs:17`
**Observed:** (Called out above.) Parse only, no consumer.

---

### [Low] `weekly_limit = Some(5_000_000)` defaults don't match the spec's example config for Alex profile

**Category:** F
**Location:** `app/src-tauri/src/config.rs:142`
**Observed:** Spec §6.2 example has alex without `weekly_limit`. Default profile in `Config::default` has `Some(5_000_000)`. Minor.

---

### [Cosmetic] `SourceEvent::AdminSnapshot.cost_today` is overwriting rather than accumulating across polls

**Category:** E
**Location:** `app/src-tauri/src/aggregator.rs:90`
**Observed:** `src.cost_today = cost_today` — since Admin API returns aggregate for today, overwrite is correct. But `input_tokens_today = tokens_today` similarly overwrites; for ccusage-backed sources the same field accumulates per-event. If a single source receives both ccusage and admin events, they will fight. Unlikely in practice (different kinds), but worth a comment.

---

### [Cosmetic] Tab order in settings UI differs slightly from spec §6.3

**Category:** D
**Observed:** Spec order: General, Profiles, Thresholds, Notifications, Sources, About — matches. Fine.

---

### [Cosmetic] Status bar "REG — · —" placeholder while registry loads may briefly flash before first state-update

**Category:** C
**Location:** `app/src/index.html:156`
**Observed:** Cosmetic; hidden behind splash for 800ms.

---

### [Cosmetic] Snap clipboard output does not include a timestamp

**Category:** D
**Location:** `app/src-tauri/src/snapshot.rs`
**Observed:** No `generated: 2026-04-22T…` line — useful for pasting into issues.

---

## Summary counts by severity

- Critical: 0
- High: 3 (cost formula / tokenizer_inflation, context %, burn rate approximation)  
  *(plus the withdrawn #5 — not counted)*
- Medium: 23
- Low: 18
- Cosmetic: 4

**Total findings: 48 (one withdrawn).**

---

## Ship recommendation

**Blockers remain** before cutting a v1.0 release candidate:

1. **Pin default mismatch** (medium) — window launches un-pinned despite config saying pinned; visible regression on first launch.
2. **Tailer offset persistence** (medium) — guaranteed data loss across restarts; contradicts spec §4.1.
3. **`cmd_set_profile` not persisted** (medium) — multi-profile users lose their selection on restart.
4. **Admin poller backoff inverted** (medium) — `.min` should be `.max`; a failing Admin API will be hammered, not backed off.
5. **Frontend cache meter thresholds disconnected from backend** (medium) — color state will lie to users.
6. **Hourly rollups table dead** (medium) — 1h/24h spectra are only accurate after 24h of uninterrupted uptime.
7. **`cmd_refresh_registry` doesn't persist** (low, but ship-relevant) — "refresh" is a runtime-only operation.

After those are addressed, the High items (cost/tokenizer_inflation, context %, burn rate) should be treated as accuracy debt and tracked; they do not prevent shipping but will surface in real usage.

The README's listed "known gaps" (real WAVs, icons, tauri build, tests, session actions) are independently required before v1.0 GA per spec §12.

---

## Smoke-test observation notes

- Cargo lockfile indicates fresh dependency graph; no test module anywhere (`#[test]` / `#[cfg(test)]` grep returns nothing in src-tauri/src).
- `tracing-subscriber` configured with env-filter `TOKEN_MAN_LOG`, default `info`. Good ergonomics for debugging.
- `panic = "abort"` in release profile — clean for a GUI app but means any remaining `.unwrap()` failure kills the process without a backtrace.
- SQLite `journal_mode = WAL, synchronous = NORMAL` — appropriate for a long-running single-writer.
- CSP is reasonably tight; `'unsafe-inline'` for style-src and script-src is needed for the inline bits in index.html.
- Frontend bundles everything in a single `main.js` at 11KB — well within the "vanilla-JS justifiable" ceiling.
- No IPC command for quit-app; only tray → Quit. Keyboard shortcut (e.g., Ctrl+Q) unbound.
- `escapeHtml` is used for model/project/kind/owner in session rows — XSS paths look clean.
- `SourceKind::ClaudeAi` exists, but `is_opaque()` includes it; spec only listed Chrome/Cowork/Excel as opaque — including claude.ai is reasonable.

---

## Not verified (requires runtime testing)

- **G. Resource usage.** Idle CPU <0.5%, memory <80MB targets from spec §3.3. Requires a built binary + perf capture. Static analysis: 500ms render tick + 1s spectrum tick + 2s ccusage safety poll + 60s admin poll = modest; the concerning path is the 500ms `recompute_metrics` which iterates all sources and runs 3–4 DB queries, all behind a write-lock on AppState — potential contention under load but unlikely to push past 0.5%.
- **Audio playback** on real Windows hardware — WAV files decode via `rodio::Decoder`; can't confirm sample rate / codec compatibility without runtime.
- **Tray icon behavior** — single-click show/hide, menu interactions on Windows vs macOS.
- **Splash → first-tick timing** — visually confirmable only at runtime.
- **VU needle ballistic animation smoothness** on 60Hz / 120Hz displays.
- **Red-mode alert-pulse CSS animation** performance on low-end GPUs.
- **Notification toast rendering** — Windows Focus Assist / quiet-hours interactions, macOS notification permissions.
- **`notify` crate behavior** on network-mounted `.claude/projects/` directories.
- **Keyring** round-trip on each platform (Windows Credential Manager, macOS Keychain, Linux Secret Service availability).
- **OTel receiver** port-scan fallback when 4318-4327 are all busy.
- **Admin API** actual response shape vs `AdminBucket` deserialization.
- **Multi-display / multi-monitor** window positioning.
- **tauri.conf.json** → `tauri build` — installer signing not yet run per README.
- **First-launch empty state** — what the HUD actually displays before any event arrives (session list says "no sources yet", but spectrum bars and meter fills will be 0 / —).
- **Clock-skew simulated tests** (set system clock backward → verify ccusage doesn't double-process).
