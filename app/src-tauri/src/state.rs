use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::db::Db;
use crate::registry::ModelRegistry;

pub type SourceId = String;
pub type ProfileId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceKind {
    Code,
    Dispatch,
    Chrome,
    Cowork,
    Excel,
    Api,
    ClaudeAi,
}

impl SourceKind {
    pub fn label(&self) -> &'static str {
        match self {
            SourceKind::Code => "code",
            SourceKind::Dispatch => "dispatch",
            SourceKind::Chrome => "chrome",
            SourceKind::Cowork => "cowork",
            SourceKind::Excel => "excel",
            SourceKind::Api => "api",
            SourceKind::ClaudeAi => "claude.ai",
        }
    }
    pub fn is_opaque(&self) -> bool {
        matches!(
            self,
            SourceKind::Chrome | SourceKind::Cowork | SourceKind::Excel | SourceKind::ClaudeAi
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    Active,
    Idle,
    Stale,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GlobalState {
    Ok,
    Warn,
    Alert,
}

impl std::fmt::Display for GlobalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            GlobalState::Ok => "OK",
            GlobalState::Warn => "WARN",
            GlobalState::Alert => "ALERT",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    ContextHigh,
    CacheLow,
    BurnHigh,
    LimitEta,
    FiveHourHigh,
    SourceOffline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub kind: AlertKind,
    pub source_id: Option<SourceId>,
    pub timestamp: DateTime<Utc>,
    pub value: f32,
    pub threshold: f32,
    pub resolved: bool,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SourceState {
    pub id: SourceId,
    pub kind: SourceKind,
    pub owner: ProfileId,
    pub model: String,
    pub project: Option<String>,
    pub context_percent: Option<f32>,
    pub tokens_per_min: Option<f32>,
    pub cost_today: f32,
    pub cache_hit_rate: Option<f32>,
    pub session_start: Option<DateTime<Utc>>,
    pub last_activity: DateTime<Utc>,
    pub is_opaque: bool,
    /// False when this source's model is absent from the model registry. Its
    /// cost could not be computed and is therefore NOT included in any total.
    /// Surfaced in the HUD so an unpriced source can never be read as a free
    /// one — the failure this app exists to catch.
    pub model_known: bool,
    pub status: SourceStatus,
    // Rolling token accounting.
    pub input_tokens_today: u64,
    pub output_tokens_today: u64,
    pub cache_read_today: u64,
    pub cache_write_today: u64,
    pub recent_ticks: VecDeque<(DateTime<Utc>, u64, u64)>, // (t, in, out)
    /// context_current_tokens = current context-window utilization at the most
    /// recent assistant turn (input + cache_read + cache_write of that turn).
    /// NOT a peak/historical value. Each Claude Code assistant turn reports
    /// its full prompt under those three fields, so the latest turn IS the
    /// current fill. Drops naturally on /compact, sub-agent boundaries, and
    /// tool-result trimming, exactly as `/context` does. (F1 fix v1.0.6.)
    pub context_current_tokens: u64,
    /// Lifetime-of-session peak; retained for telemetry / future diagnostics.
    /// Do NOT surface this in the HUD — the prior bug in v1.0.5 was using it
    /// as the displayed CTX %, which pinned the meter to a historical value
    /// after /compact.
    pub context_peak_tokens: u64,
}

impl SourceState {
    pub fn new(id: SourceId, kind: SourceKind, owner: ProfileId) -> Self {
        let now = Utc::now();
        Self {
            id,
            kind,
            owner,
            model: String::new(),
            project: None,
            context_percent: None,
            tokens_per_min: None,
            cost_today: 0.0,
            cache_hit_rate: None,
            session_start: Some(now),
            last_activity: now,
            is_opaque: kind.is_opaque(),
            model_known: true,
            status: SourceStatus::Active,
            input_tokens_today: 0,
            output_tokens_today: 0,
            cache_read_today: 0,
            cache_write_today: 0,
            recent_ticks: VecDeque::with_capacity(256),
            context_current_tokens: 0,
            context_peak_tokens: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RingBuffer<T: Clone> {
    pub buf: VecDeque<T>,
    pub cap: usize,
}

impl<T: Clone> RingBuffer<T> {
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }
    pub fn push(&mut self, v: T) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.buf.iter()
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

pub struct GlobalMetrics {
    pub tokens_per_min_current: f32,
    pub tokens_per_sec_vu: f32,
    pub live_sessions: u32,
    pub cost_today: f32,
    pub limit_eta_min: Option<u32>,
    pub burn_rate_per_hour: f32,
    pub cache_hit_rate_global: f32,
    pub context_percent_worst: f32,
    pub five_hour_usage_percent: f32,
    pub cache_usage_percent: f32,
    pub week_usage_percent: f32,
    pub spectrum_60s: RingBuffer<(f32, f32)>,
    pub spectrum_1h: RingBuffer<(f32, f32)>,
    pub spectrum_24h: RingBuffer<(f32, f32)>,
    /// EWMA-smoothed burn $/hr (F3 fix v1.0.6). See canonical-definition
    /// comment at the computation site in aggregator::recompute_metrics.
    /// `None` until first sample is seeded.
    pub burn_rate_ewma: Option<f32>,
}

impl Default for GlobalMetrics {
    fn default() -> Self {
        Self {
            tokens_per_min_current: 0.0,
            tokens_per_sec_vu: 0.0,
            live_sessions: 0,
            cost_today: 0.0,
            limit_eta_min: None,
            burn_rate_per_hour: 0.0,
            cache_hit_rate_global: 0.0,
            context_percent_worst: 0.0,
            five_hour_usage_percent: 0.0,
            cache_usage_percent: 0.0,
            week_usage_percent: 0.0,
            spectrum_60s: RingBuffer::new(40),
            spectrum_1h: RingBuffer::new(40),
            spectrum_24h: RingBuffer::new(40),
            burn_rate_ewma: None,
        }
    }
}

pub struct AppState {
    pub sources: HashMap<SourceId, SourceState>,
    pub global: GlobalMetrics,
    pub alerts: VecDeque<Alert>,
    pub global_state: GlobalState,
    pub active_profile: ProfileId,
    pub pin_active: bool,
    pub spectrum_window: SpectrumWindow,
    pub registry: ModelRegistry,
    pub last_jsonl_event: Option<DateTime<Utc>>,
    /// Cached per-profile metadata (id → (name, color)) so build_view can
    /// render the active profile's name/color without holding a config lock.
    /// Populated at startup from Config and refreshed by config writes.
    pub profile_meta: HashMap<ProfileId, (String, String)>,
    pub cache_warn_threshold: f32,
    pub cache_critical_threshold: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpectrumWindow {
    #[serde(rename = "60s")]
    S60,
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "24h")]
    H24,
}

impl Default for SpectrumWindow {
    fn default() -> Self {
        SpectrumWindow::S60
    }
}

impl AppState {
    pub fn new(config: &Config, _db: &Db) -> anyhow::Result<Self> {
        let registry = ModelRegistry::load_bundled()?;
        let profile_meta: HashMap<ProfileId, (String, String)> = config
            .profiles
            .iter()
            .map(|p| (p.id.clone(), (p.name.clone(), p.color.clone())))
            .collect();
        // F12 fix v1.0.6: emit the active-profile-mismatch warning once at
        // startup with resolution detail. The previous per-tick warn was
        // log spam (~2/s for the lifetime of the process).
        if !profile_meta.contains_key(&config.app.active_profile) {
            let resolved = config
                .profiles
                .first()
                .map(|p| p.id.as_str())
                .unwrap_or("<no profiles>");
            tracing::warn!(
                target: "config",
                "active_profile '{}' not found in [profiles]; falling back to '{}' for this session. Edit config.toml to fix.",
                config.app.active_profile,
                resolved
            );
        }
        Ok(Self {
            sources: HashMap::new(),
            global: GlobalMetrics::default(),
            alerts: VecDeque::with_capacity(128),
            global_state: GlobalState::Ok,
            active_profile: config.app.active_profile.clone(),
            pin_active: config.app.always_on_top_default,
            spectrum_window: SpectrumWindow::S60,
            registry,
            last_jsonl_event: None,
            profile_meta,
            cache_warn_threshold: config.thresholds.cache_warn,
            cache_critical_threshold: config.thresholds.cache_critical,
        })
    }

    pub fn build_view(&self) -> StateUpdate {
        let sources: Vec<SourceView> = self
            .sources
            .values()
            .map(|s| SourceView {
                id: s.id.clone(),
                kind: s.kind,
                owner: s.owner.clone(),
                model: s.model.clone(),
                project: s.project.clone(),
                context_percent: s.context_percent,
                tokens_per_min: s.tokens_per_min,
                cost_today: s.cost_today,
                cache_hit_rate: s.cache_hit_rate,
                is_opaque: s.is_opaque,
                model_known: s.model_known,
                status: s.status,
            })
            .collect();

        // Any unpriced source means every cost figure below is a floor.
        let cost_incomplete = sources.iter().any(|s| !s.model_known);

        let spectrum = match self.spectrum_window {
            SpectrumWindow::S60 => &self.global.spectrum_60s,
            SpectrumWindow::H1 => &self.global.spectrum_1h,
            SpectrumWindow::H24 => &self.global.spectrum_24h,
        };
        let max = spectrum
            .iter()
            .map(|(a, b)| a.max(*b))
            .fold(1.0_f32, f32::max);
        let in_bars: Vec<f32> = spectrum.iter().map(|(i, _)| (i / max).min(1.0)).collect();
        let out_bars: Vec<f32> = spectrum.iter().map(|(_, o)| (o / max).min(1.0)).collect();

        let alerts: Vec<AlertView> = self
            .alerts
            .iter()
            .rev()
            .take(8)
            .map(|a| AlertView {
                id: a.id.clone(),
                kind: a.kind,
                source_id: a.source_id.clone(),
                value: a.value,
                threshold: a.threshold,
                resolved: a.resolved,
            })
            .collect();

        StateUpdate {
            global_state: self.global_state,
            metrics: MetricsView {
                tokens_per_min: self.global.tokens_per_min_current,
                tokens_per_sec: self.global.tokens_per_sec_vu,
                live_sessions: self.global.live_sessions,
                cost_today: self.global.cost_today,
                limit_eta: self.global.limit_eta_min.map(format_eta),
                burn_per_hour: self.global.burn_rate_per_hour,
                cache_hit_rate: self.global.cache_hit_rate_global,
                ctx_worst: self.global.context_percent_worst,
                five_hour: self.global.five_hour_usage_percent,
                week: self.global.week_usage_percent,
                cost_incomplete,
            },
            sources,
            spectrum: SpectrumView {
                window: self.spectrum_window,
                r#in: in_bars,
                out: out_bars,
            },
            alerts,
            profile: {
                // F12 fix v1.0.6: profile lookup mirrors
                // Config::active_profile() resolution semantics: try exact id
                // match, else fall back to the first profile in the list.
                // The not-found warning is emitted ONCE at startup (see
                // AppState::new), not on every render tick.
                let (id, name, color) = self
                    .profile_meta
                    .get(&self.active_profile)
                    .map(|(n, c)| (self.active_profile.clone(), n.clone(), c.clone()))
                    .or_else(|| {
                        self.profile_meta
                            .iter()
                            .next()
                            .map(|(id, (n, c))| (id.clone(), n.clone(), c.clone()))
                    })
                    .unwrap_or_else(|| {
                        (
                            self.active_profile.clone(),
                            self.active_profile.clone(),
                            "#378ADD".into(),
                        )
                    });
                ProfileView { id, name, color }
            },
            registry: RegistryView {
                version: self.registry.version.clone(),
                updated_at: self.registry.updated.clone(),
            },
            pin_active: self.pin_active,
            thresholds: ThresholdsView {
                cache_warn: self.cache_warn_threshold,
                cache_critical: self.cache_critical_threshold,
            },
        }
    }
}

fn format_eta(min: u32) -> String {
    if min >= 24 * 60 {
        "24H+".into()
    } else if min >= 60 {
        let h = min / 60;
        let m = min % 60;
        format!("{h}h {m}m")
    } else {
        format!("{min}m")
    }
}

// -------- View types emitted to the webview --------

#[derive(Debug, Clone, Serialize)]
pub struct StateUpdate {
    #[serde(rename = "globalState")]
    pub global_state: GlobalState,
    pub metrics: MetricsView,
    pub sources: Vec<SourceView>,
    pub spectrum: SpectrumView,
    pub alerts: Vec<AlertView>,
    pub profile: ProfileView,
    pub registry: RegistryView,
    #[serde(rename = "pinActive")]
    pub pin_active: bool,
    pub thresholds: ThresholdsView,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThresholdsView {
    #[serde(rename = "cacheWarn")]
    pub cache_warn: f32,
    #[serde(rename = "cacheCritical")]
    pub cache_critical: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsView {
    #[serde(rename = "tokensPerMin")]
    pub tokens_per_min: f32,
    #[serde(rename = "tokensPerSec")]
    pub tokens_per_sec: f32,
    #[serde(rename = "liveSessions")]
    pub live_sessions: u32,
    #[serde(rename = "costToday")]
    pub cost_today: f32,
    #[serde(rename = "limitEta")]
    pub limit_eta: Option<String>,
    #[serde(rename = "burnPerHour")]
    pub burn_per_hour: f32,
    #[serde(rename = "cacheHitRate")]
    pub cache_hit_rate: f32,
    #[serde(rename = "ctxWorst")]
    pub ctx_worst: f32,
    #[serde(rename = "fiveHour")]
    pub five_hour: f32,
    pub week: f32,
    /// True when at least one live source ran a model the registry does not
    /// price. `cost_today` is then a floor, not a total.
    #[serde(rename = "costIncomplete")]
    pub cost_incomplete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceView {
    pub id: String,
    pub kind: SourceKind,
    pub owner: String,
    pub model: String,
    pub project: Option<String>,
    #[serde(rename = "contextPercent")]
    pub context_percent: Option<f32>,
    #[serde(rename = "tokensPerMin")]
    pub tokens_per_min: Option<f32>,
    #[serde(rename = "costToday")]
    pub cost_today: f32,
    #[serde(rename = "cacheHitRate")]
    pub cache_hit_rate: Option<f32>,
    #[serde(rename = "isOpaque")]
    pub is_opaque: bool,
    #[serde(rename = "modelKnown")]
    pub model_known: bool,
    pub status: SourceStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpectrumView {
    pub window: SpectrumWindow,
    pub r#in: Vec<f32>,
    pub out: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertView {
    pub id: String,
    pub kind: AlertKind,
    #[serde(rename = "sourceId")]
    pub source_id: Option<String>,
    pub value: f32,
    pub threshold: f32,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryView {
    pub version: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}
