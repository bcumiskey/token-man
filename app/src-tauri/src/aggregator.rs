//! Aggregator — consumes source events, updates in-memory state, persists to
//! SQLite, evaluates thresholds, and relays commands.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Duration as ChronoDuration, Local, TimeZone, Utc};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::db::{Db, EventRow};
use crate::ipc::BackendCommand;
use crate::sources::{SourceEvent, UsageEvent};
use crate::state::{AppState, SourceState, SourceStatus};

pub struct Aggregator {
    state: Arc<RwLock<AppState>>,
    db: Arc<Db>,
    config: Arc<RwLock<Config>>,
    handle: AppHandle,
}

impl Aggregator {
    pub fn new(
        state: Arc<RwLock<AppState>>,
        db: Arc<Db>,
        config: Arc<RwLock<Config>>,
        handle: AppHandle,
    ) -> Self {
        Self { state, db, config, handle }
    }

    pub async fn run(
        self,
        mut event_rx: mpsc::Receiver<SourceEvent>,
        mut command_rx: mpsc::Receiver<BackendCommand>,
    ) {
        // Seed spectrum_1h and spectrum_24h from persisted events so the 1H
        // and 24H window views aren't blank for the first hour/day of uptime.
        self.seed_spectrum_from_db().await;

        let mut ticker = interval(Duration::from_millis(500));
        let mut spectrum_tick = interval(Duration::from_secs(1));
        let mut rollup_tick = interval(Duration::from_secs(300));
        info!("aggregator running");
        loop {
            tokio::select! {
                Some(ev) = event_rx.recv() => {
                    self.handle_event(ev).await;
                }
                Some(cmd) = command_rx.recv() => {
                    self.handle_command(cmd).await;
                }
                _ = ticker.tick() => {
                    self.recompute_metrics().await;
                }
                _ = spectrum_tick.tick() => {
                    self.advance_spectrum().await;
                }
                _ = rollup_tick.tick() => {
                    let db = self.db.clone();
                    tokio::task::spawn_blocking(move || {
                        let now = Utc::now().timestamp_millis();
                        if let Err(e) = db.rollup_hours(now) {
                            debug!("hourly rollup failed: {e:?}");
                        }
                    });
                }
            }
        }
    }

    async fn seed_spectrum_from_db(&self) {
        let now = Utc::now().timestamp_millis();
        // 1H ring: 40 bars × 90s buckets (covers last hour).
        if let Ok(rows) = self.db.bucket_tokens(now, 90_000, 40) {
            let mut s = self.state.write().await;
            for (_b, i, o) in rows {
                s.global.spectrum_1h.push((i as f32, o as f32));
            }
        }
        // 24H ring: 40 bars × 36-minute buckets (covers last day).
        if let Ok(rows) = self.db.bucket_tokens(now, 36 * 60_000, 40) {
            let mut s = self.state.write().await;
            for (_b, i, o) in rows {
                s.global.spectrum_24h.push((i as f32, o as f32));
            }
        }
    }

    async fn handle_event(&self, ev: SourceEvent) {
        match ev {
            SourceEvent::Usage(u) => self.handle_usage(u).await,
            SourceEvent::SessionStart { source_id, kind, project, model } => {
                let mut s = self.state.write().await;
                let ss = s.sources.entry(source_id.clone()).or_insert_with(|| {
                    SourceState::new(source_id.clone(), kind, "you".into())
                });
                ss.project = project;
                if let Some(m) = model {
                    ss.model = m;
                }
                ss.status = SourceStatus::Active;
            }
            SourceEvent::StatusChange { source_id, status } => {
                let mut s = self.state.write().await;
                if let Some(src) = s.sources.get_mut(&source_id) {
                    src.status = status;
                }
            }
            SourceEvent::AdminSnapshot { kind, owner, cost_today, tokens_today } => {
                let source_id = format!("{}:{}", kind.label(), owner);
                let mut s = self.state.write().await;
                let src = s.sources.entry(source_id.clone()).or_insert_with(|| {
                    SourceState::new(source_id.clone(), kind, owner.clone())
                });
                src.cost_today = cost_today;
                src.input_tokens_today = tokens_today;
                src.last_activity = Utc::now();
                src.status = SourceStatus::Active;
                src.is_opaque = true;
            }
        }
    }

    async fn handle_usage(&self, u: UsageEvent) {
        let registry = { self.state.read().await.registry.clone() };
        let cost = registry.cost(
            &u.model,
            u.input_tokens,
            u.output_tokens,
            u.cache_read_tokens,
            u.cache_write_tokens,
        );
        if cost.is_none() {
            // Unknown model: record NULL, not 0.0. A zero here would be
            // indistinguishable from a genuinely free turn and would silently
            // drag every daily total down. The warning is the signal to add
            // the model to assets/model-registry.json.
            warn!(
                "unknown model {:?} — cost not computed for this turn; add it to assets/model-registry.json",
                u.model
            );
        }

        // Persist.
        let row = EventRow {
            source_id: u.source_id.clone(),
            timestamp: u.timestamp.timestamp_millis(),
            event_type: "token_usage".into(),
            model: Some(u.model.clone()),
            input_tokens: Some(u.input_tokens as i64),
            output_tokens: Some(u.output_tokens as i64),
            cache_read_tokens: Some(u.cache_read_tokens as i64),
            cache_write_tokens: Some(u.cache_write_tokens as i64),
            cost_usd: cost.map(|c| c as f64),
            metadata: u.project.clone(),
        };
        if let Err(e) = self.db.insert_event(&row) {
            debug!("db insert failed: {e:?}");
        }

        // Use wall-clock `now`, not the JSONL-declared timestamp, so clock
        // skew or late-arriving events don't poison rate/idle windows.
        let now = Utc::now();

        // Update in-memory.
        let mut s = self.state.write().await;
        s.last_jsonl_event = Some(u.timestamp);
        let src = s
            .sources
            .entry(u.source_id.clone())
            .or_insert_with(|| SourceState::new(u.source_id.clone(), u.kind, u.owner.clone()));
        src.model = u.model.clone();
        src.model_known = registry.is_known(&u.model);
        src.project = u.project.clone();
        src.owner = u.owner.clone();
        src.input_tokens_today += u.input_tokens;
        src.output_tokens_today += u.output_tokens;
        src.cache_read_today += u.cache_read_tokens;
        src.cache_write_today += u.cache_write_tokens;
        // src.cost_today is no longer accumulated here — it is derived in
        // recompute_metrics from DB SUM since local-midnight (F4 v1.0.6).
        // recompute_metrics overwrites it each tick, so any stale
        // process-lifetime accumulator from a pre-1.0.6 in-memory state is
        // corrected on the first tick after launch (the F4 one-shot
        // migration is implicit — the in-memory value is never persisted).
        src.last_activity = now;
        src.status = SourceStatus::Active;

        // CANONICAL DEFINITION (CTX %) — F1 fix v1.0.6
        //
        // context_current_tokens = current context-window utilization at the
        // most recent assistant turn. NOT a peak/historical value.
        //
        // Each Claude Code assistant turn reports its full prompt as
        // (input + cache_read + cache_write) under usage. The LATEST turn's
        // sum IS the current context fill — exactly what `/context` reports.
        // Reset semantics: writes are unconditional per turn, so /compact,
        // sub-agent boundaries, and natural tool-result trimming are all
        // honored automatically (next turn after the contraction reports the
        // smaller value, and we display it). Session boundary: a new
        // source_id implies a fresh SourceState with both fields = 0.
        // The displayed % uses tokenizer_inflation so it matches /context.
        if let Some(ctx_tokens) = u.context_tokens {
            src.context_current_tokens = ctx_tokens;
            if ctx_tokens > src.context_peak_tokens {
                src.context_peak_tokens = ctx_tokens;
            }
        }
        if let Some(window) = registry.context_window(&u.model) {
            let inflation = registry.tokenizer_inflation(&u.model);
            let inflated = (src.context_current_tokens as f32) * inflation;
            let pct = (inflated / window as f32) * 100.0;
            src.context_percent = Some(pct);
            info!(
                target: "ctx_diag",
                "CTX_DIAG | source_id={} | file_path={} | ts={} | model={} | input={} | output={} | cache_read={} | cache_write={} | ctx_raw={} | ctx_current_raw={} | ctx_peak_raw={} | inflation={:.3} | ctx_adjusted={:.0} | window={} | pct={:.2} | sidechain={:?} | turn={:?}",
                u.source_id,
                u.file_path.as_deref().unwrap_or("-"),
                u.timestamp.to_rfc3339(),
                u.model,
                u.input_tokens,
                u.output_tokens,
                u.cache_read_tokens,
                u.cache_write_tokens,
                u.context_tokens.unwrap_or(0),
                src.context_current_tokens,
                src.context_peak_tokens,
                inflation,
                inflated,
                window,
                pct,
                u.is_sidechain,
                u.turn_type,
            );
        }

        // "Input" for the rate ring = new inbound work only (prompt + cache
        // writes). cache_read represents existing context being re-processed
        // each turn; including it credits a full 150k context to a single 1s
        // bucket and inflates tok/min dramatically. Cache reads still count
        // toward context % (see context_current_tokens above).
        let in_total = u.input_tokens + u.cache_write_tokens;
        src.recent_ticks.push_back((now, in_total, u.output_tokens));
        let cutoff = now - ChronoDuration::seconds(60);
        while src
            .recent_ticks
            .front()
            .map(|(t, _, _)| *t < cutoff)
            .unwrap_or(false)
        {
            src.recent_ticks.pop_front();
        }

        let cache_total = src.cache_read_today + src.cache_write_today;
        let input_total = src.input_tokens_today + cache_total;
        if input_total > 0 {
            src.cache_hit_rate = Some((src.cache_read_today as f32 / input_total as f32) * 100.0);
        }
    }

    async fn recompute_metrics(&self) {
        let now = Utc::now();
        let (thresholds, five_hour_limit, weekly_limit) = {
            let c = self.config.read().await;
            let p = c.active_profile();
            (
                c.thresholds.clone(),
                p.map(|p| p.five_hour_limit).unwrap_or(200_000),
                p.and_then(|p| p.weekly_limit).unwrap_or(5_000_000),
            )
        };

        // CANONICAL DEFINITION (cost_today) — F4 fix v1.0.6
        //
        // cost_today = SUM(cost_usd) over events with timestamp ≥ local
        // midnight of the user's machine. Resets every wall-clock day.
        // TZ choice: chrono::Local — the user's machine local time. This is
        // the least-surprising default for an HUD watched on the same
        // machine that's running Claude Code; if a user works across
        // timezones, "today" still tracks the calendar they see on their
        // taskbar clock. UTC was rejected because users in non-UTC zones
        // would see the meter reset at unintuitive hours.
        // Migration: derived from DB on every tick, so the v1.0.5
        // process-lifetime accumulator is replaced automatically on the
        // first launch of v1.0.6 — no schema change, no one-shot script.
        let local_midnight_ms = {
            let now_local = Local::now();
            let midnight_local = Local
                .with_ymd_and_hms(
                    now_local.year(),
                    now_local.month(),
                    now_local.day(),
                    0,
                    0,
                    0,
                )
                .single()
                .unwrap_or(now_local);
            midnight_local.timestamp_millis()
        };
        let cost_by_source = self
            .db
            .cost_by_source_since(local_midnight_ms)
            .unwrap_or_default();

        let mut s = self.state.write().await;

        // Apply per-source cost_today from DB (local-midnight bound).
        for src in s.sources.values_mut() {
            src.cost_today = 0.0;
        }
        for (sid, c) in &cost_by_source {
            if let Some(src) = s.sources.get_mut(sid) {
                src.cost_today = *c as f32;
            }
        }
        // Global cost_today is the SUM across sources (from DB) — derive
        // independently so a source that hasn't yet been registered in the
        // sources map is still counted.
        let cost_today_global: f32 =
            cost_by_source.iter().map(|(_, c)| *c as f32).sum();

        // Live sessions: any source with activity in last 60s.
        let live_cutoff = now - ChronoDuration::seconds(60);
        let stale_cutoff = now - ChronoDuration::minutes(10);
        let mut live = 0u32;
        let mut worst_ctx = 0f32;
        let mut t_per_min_sum = 0f32;
        let mut t_per_sec_short = 0f32;
        let mut cache_hit_weighted = 0f32;
        let mut cache_hit_denom = 0f32;

        for src in s.sources.values_mut() {
            // Prune stale ticks every recompute so idle sources show 0 tok/min
            // rather than the frozen last burst. Without this, the rate buffer
            // only evicts on new events, which keeps ETA/burn elevated for
            // minutes after activity ends.
            while src
                .recent_ticks
                .front()
                .map(|(t, _, _)| *t < live_cutoff)
                .unwrap_or(false)
            {
                src.recent_ticks.pop_front();
            }

            // Status decay.
            if src.last_activity < stale_cutoff {
                src.status = SourceStatus::Stale;
            } else if src.last_activity < live_cutoff {
                src.status = SourceStatus::Idle;
            } else if !matches!(src.status, SourceStatus::Offline) {
                src.status = SourceStatus::Active;
                live += 1;
            }

            // Tokens/min over last 60s — instantaneous rate.
            let recent_sum: u64 = src.recent_ticks.iter().map(|(_, i, o)| i + o).sum();
            let tpm = recent_sum as f32;
            src.tokens_per_min = Some(tpm);
            t_per_min_sum += tpm;

            // Short window (last 2s) for VU — gives a live, responsive pulse.
            let two_sec = now - ChronoDuration::seconds(2);
            let short_sum: u64 = src
                .recent_ticks
                .iter()
                .filter(|(t, _, _)| *t >= two_sec)
                .map(|(_, i, o)| i + o)
                .sum();
            t_per_sec_short += short_sum as f32 / 2.0;

            if let Some(ctx) = src.context_percent {
                if ctx > worst_ctx {
                    worst_ctx = ctx;
                }
            }
            if let Some(ch) = src.cache_hit_rate {
                let weight = (src.input_tokens_today + src.cache_read_today) as f32;
                cache_hit_weighted += ch * weight;
                cache_hit_denom += weight;
            }
        }

        s.global.live_sessions = live;
        s.global.tokens_per_min_current = t_per_min_sum;
        s.global.tokens_per_sec_vu = t_per_sec_short;
        s.global.cost_today = cost_today_global;
        s.global.context_percent_worst = worst_ctx;
        s.global.cache_hit_rate_global = if cache_hit_denom > 0.0 {
            cache_hit_weighted / cache_hit_denom
        } else {
            0.0
        };

        // CANONICAL DEFINITION (5h% / week%) — verified 2026-04-28
        //
        // Categories included in the numerator (each at full weight, 1.0×):
        //   input_tokens + output_tokens + cache_read_tokens + cache_write_tokens
        //
        // Anthropic's published 5-hour and weekly subscription rate limits meter
        // *all* token traffic the model serves, including cache hits and cache
        // creations. Audit AUDIT-v1.0.5-FINDINGS.md §F2 documents that the prior
        // (input + output) basis under-counted by ~5 orders of magnitude on
        // heavy-cache opus sessions — a session reporting "5%" was in fact
        // > 100%. Verification source for the inclusive interpretation:
        // Anthropic docs on Pro/Team/Max usage limits + prompt caching
        // (consult anthropic.com/pricing and docs.anthropic.com prompt-caching
        // pages on next online run; offline at fix time, so we picked the
        // INCLUSIVE interpretation per release brief — under-counting is the
        // dangerous failure mode here).
        // No multipliers are applied (1.0× across categories). If Anthropic's
        // future docs clarify weighted billing, adjust here in one place.
        let five_hour_cutoff = (now - ChronoDuration::hours(5)).timestamp_millis();
        if let Ok((i, o, cr, cw)) = self.db.sum_all_tokens_since(five_hour_cutoff) {
            let used = (i + o + cr + cw) as f32;
            s.global.five_hour_usage_percent = (used / five_hour_limit as f32) * 100.0;
        }
        let week_cutoff = (now - ChronoDuration::days(7)).timestamp_millis();
        if let Ok((i, o, cr, cw)) = self.db.sum_all_tokens_since(week_cutoff) {
            let used = (i + o + cr + cw) as f32;
            s.global.week_usage_percent = (used / weekly_limit as f32) * 100.0;
        }
        s.global.cache_usage_percent = s.global.cache_hit_rate_global;

        // CANONICAL DEFINITION (burn $/hr) — F3 fix v1.0.6
        //
        // Underlying definition is unchanged from v1.0.5: instantaneous rate
        // is SUM(cost_usd over last 10 min) × 6. We keep the same window
        // (10 min) and same scaling (×6). The displayed value is then passed
        // through an EWMA smoother before publishing, to absorb single
        // large-cache turns that previously made burn spike for a full 10 min
        // window.
        //
        // Smoothing choice: EWMA with α = 0.02, applied on each 500 ms recompute
        // tick (≈ 2 Hz). The resulting time constant τ = (Δt / α) ≈ 25 s, so a
        // single one-shot $1.50 cost (which under raw v1.0.5 would jump burn by
        // ~$9/hr) decays to half its peak contribution in ~17 s instead of
        // pinning at full deflection for 10 min, while a sustained burst that
        // lasts ≥ 30 s still reads close to its true raw rate. EWMA chosen over
        // median-of-N because it (a) keeps the existing 10-min rolling-window
        // semantics for the underlying SUM, (b) needs only one f32 of state,
        // (c) responds smoothly rather than stepping. Parameters live here;
        // tune in one place.
        const BURN_EWMA_ALPHA: f32 = 0.02;
        let ten_min_cutoff = (now - ChronoDuration::minutes(10)).timestamp_millis();
        if let Ok(recent_cost) = self.db.sum_cost_since(ten_min_cutoff) {
            let raw = (recent_cost as f32) * 6.0;
            // Seed from 0 rather than raw so the first tick doesn't bypass
            // smoothing. Cost is monotonic over short timespans; ramp-up to
            // steady state takes ~τ = 25 s, which is acceptable on launch.
            let prev = s.global.burn_rate_ewma.unwrap_or(0.0);
            let smoothed = prev + BURN_EWMA_ALPHA * (raw - prev);
            s.global.burn_rate_ewma = Some(smoothed);
            s.global.burn_rate_per_hour = smoothed;
        }

        // Limit ETA: tokens remaining in 5h window ÷ sustained tokens/min.
        // Require a meaningful burn rate (≥100 tok/min) — below that,
        // extrapolation is noise and the UI should show "—".
        let five_hour_pct = s.global.five_hour_usage_percent;
        let tpm = s.global.tokens_per_min_current;
        const TPM_FLOOR: f32 = 100.0;
        const ETA_CAP_MIN: u32 = 24 * 60;
        if tpm < TPM_FLOOR || five_hour_pct >= 100.0 || five_hour_limit == 0 {
            s.global.limit_eta_min = None;
        } else {
            let remaining = (1.0 - five_hour_pct / 100.0) * five_hour_limit as f32;
            let mins = (remaining / tpm).max(0.0) as u32;
            s.global.limit_eta_min = Some(mins.min(ETA_CAP_MIN));
        }
        // Log ETA inputs once per minute.
        static ETA_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let c = ETA_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if c % 120 == 0 {
            debug!(
                "eta calc: tpm={:.1} 5h%={:.1} limit={} eta={:?}",
                tpm, five_hour_pct, five_hour_limit, s.global.limit_eta_min
            );
        }

        let new_alerts = crate::thresholds::evaluate(&mut s, &thresholds);
        drop(s);

        for a in new_alerts {
            self.fire_notification(&a).await;
        }
    }

    async fn advance_spectrum(&self) {
        let mut s = self.state.write().await;
        let now = Utc::now();
        let mut sum_in = 0f32;
        let mut sum_out = 0f32;
        for src in s.sources.values() {
            let one_sec = now - ChronoDuration::seconds(1);
            for (t, i, o) in src.recent_ticks.iter() {
                if *t >= one_sec {
                    sum_in += *i as f32;
                    sum_out += *o as f32;
                }
            }
        }
        s.global.spectrum_60s.push((sum_in, sum_out));

        // Downsample into 1h and 24h buckets on a schedule.
        // Simple strategy: every 90s, push a 1h sample; every 36min, push a 24h sample.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let c = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if c % 90 == 0 {
            // 1h bar = sum over last 90 seconds.
            let bars = &s.global.spectrum_60s.buf;
            let take = bars.iter().rev().take(90);
            let (i, o) = take.fold((0f32, 0f32), |(a, b), (x, y)| (a + x, b + y));
            s.global.spectrum_1h.push((i, o));
        }
        if c % 2160 == 0 {
            let bars = &s.global.spectrum_1h.buf;
            let (i, o) = bars.iter().fold((0f32, 0f32), |(a, b), (x, y)| (a + x, b + y));
            s.global.spectrum_24h.push((i, o));
        }
    }

    async fn fire_notification(&self, a: &crate::state::Alert) {
        use tauri_plugin_notification::NotificationExt;
        let (title, body) = match a.kind {
            crate::state::AlertKind::ContextHigh => (
                "Token-Man — context high",
                format!("Context at {:.0}% — /compact recommended", a.value),
            ),
            crate::state::AlertKind::CacheLow => (
                "Token-Man — cache hit rate low",
                format!("Cache at {:.0}% — cold caches cost real money", a.value),
            ),
            crate::state::AlertKind::BurnHigh => (
                "Token-Man — burn rate high",
                format!("${:.2}/hr — check what's running", a.value),
            ),
            crate::state::AlertKind::LimitEta => (
                "Token-Man — approaching limit",
                format!("{:.0}m until 5-hour window fills", a.value),
            ),
            crate::state::AlertKind::FiveHourHigh => (
                "Token-Man — 5-hour window",
                format!("{:.0}% of 5-hour window used", a.value),
            ),
            crate::state::AlertKind::SourceOffline => (
                "Token-Man — source offline",
                format!("A source went offline"),
            ),
        };
        let cfg = self.config.read().await;
        if cfg.app.notification_toasts {
            let _ = self
                .handle
                .notification()
                .builder()
                .title(title)
                .body(body.clone())
                .show();
        }
        if cfg.app.notification_sound {
            crate::audio::play_alert_async().await;
        }
        let _ = self.handle.emit("alert-fired", a);
    }

    async fn handle_command(&self, cmd: BackendCommand) {
        match cmd {
            BackendCommand::SetSpectrumWindow(w) => {
                let mut s = self.state.write().await;
                s.spectrum_window = w;
            }
            BackendCommand::SelectSession(id) => {
                debug!("select session {id}");
            }
            BackendCommand::TestAlert => {
                let alert = crate::state::Alert {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: crate::state::AlertKind::ContextHigh,
                    source_id: None,
                    timestamp: chrono::Utc::now(),
                    value: 99.0,
                    threshold: 85.0,
                    resolved: false,
                    resolved_at: None,
                };
                self.fire_notification(&alert).await;
            }
        }
    }
}

