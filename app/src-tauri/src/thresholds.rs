use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::config::Thresholds;
use crate::state::{Alert, AlertKind, AppState, GlobalState};

/// Hysteresis: a condition must remain clear for this many consecutive ticks
/// before the alert is marked resolved. Prevents flapping near thresholds.
const CLEAR_TICKS_REQUIRED: u32 = 3;

type ClearKey = (AlertKind, Option<String>);
fn clear_counters() -> &'static Mutex<HashMap<ClearKey, u32>> {
    static C: OnceLock<Mutex<HashMap<ClearKey, u32>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Evaluate thresholds and update `AppState.global_state` + alert ring. Returns
/// the list of alerts that transitioned from not-firing to firing.
pub fn evaluate(state: &mut AppState, t: &Thresholds) -> Vec<Alert> {
    let mut new_alerts = Vec::new();

    check(
        state,
        &mut new_alerts,
        AlertKind::ContextHigh,
        None,
        Some(state.global.context_percent_worst),
        t.ctx_warn,
        t.ctx_critical,
        |v, thr| v >= thr,
    );

    check(
        state,
        &mut new_alerts,
        AlertKind::FiveHourHigh,
        None,
        Some(state.global.five_hour_usage_percent),
        t.five_hour_warn,
        t.five_hour_critical,
        |v, thr| v >= thr,
    );

    check(
        state,
        &mut new_alerts,
        AlertKind::CacheLow,
        None,
        Some(state.global.cache_hit_rate_global),
        t.cache_warn,
        t.cache_critical,
        |v, thr| v <= thr && v > 0.0,
    );

    check(
        state,
        &mut new_alerts,
        AlertKind::BurnHigh,
        None,
        Some(state.global.burn_rate_per_hour),
        t.burn_warn,
        t.burn_critical,
        |v, thr| v >= thr,
    );

    // ETA: when None (burn too low to extrapolate), pass None so any existing
    // unresolved alert gets resolved.
    let eta_val = state.global.limit_eta_min.map(|e| e as f32);
    check(
        state,
        &mut new_alerts,
        AlertKind::LimitEta,
        None,
        eta_val,
        t.eta_warn_min as f32,
        t.eta_critical_min as f32,
        |v, thr| v <= thr,
    );

    // Global state is derived purely from currently unresolved alerts.
    state.global_state = compute_global_state(state);

    new_alerts
}

fn compute_global_state(state: &AppState) -> GlobalState {
    let mut worst = GlobalState::Ok;
    for a in state.alerts.iter() {
        if a.resolved {
            continue;
        }
        // Severity inferred from threshold comparison stored on the alert.
        // For Alert vs Warn, we rely on whether the alert was created at the
        // critical threshold; we don't re-derive here — any unresolved alert
        // is at least Warn, and if its stored threshold corresponds to the
        // critical tier the caller records that by whatever ordering; simplify
        // by treating unresolved alerts as Warn unless the value is beyond
        // 10% past threshold in the severity direction.
        let intense = match a.kind {
            AlertKind::CacheLow | AlertKind::LimitEta => a.value <= a.threshold * 0.9,
            _ => a.value >= a.threshold * 1.1,
        };
        let s = if intense { GlobalState::Alert } else { GlobalState::Warn };
        if matches!((s, worst), (GlobalState::Alert, _) | (GlobalState::Warn, GlobalState::Ok)) {
            worst = s;
        }
    }
    worst
}

#[allow(clippy::too_many_arguments)]
fn check(
    state: &mut AppState,
    new_alerts: &mut Vec<Alert>,
    kind: AlertKind,
    source_id: Option<String>,
    value: Option<f32>,
    warn_t: f32,
    crit_t: f32,
    cmp: impl Fn(f32, f32) -> bool,
) {
    let severity = value.and_then(|v| {
        if cmp(v, crit_t) {
            Some((GlobalState::Alert, crit_t))
        } else if cmp(v, warn_t) {
            Some((GlobalState::Warn, warn_t))
        } else {
            None
        }
    });

    let pre_existing = state
        .alerts
        .iter()
        .any(|a| a.kind == kind && a.source_id == source_id && !a.resolved);

    let key: ClearKey = (kind, source_id.clone());
    let mut counters = clear_counters().lock().unwrap();

    match severity {
        Some((_, thr)) => {
            counters.remove(&key);
            if !pre_existing {
                let a = Alert {
                    id: Uuid::new_v4().to_string(),
                    kind,
                    source_id: source_id.clone(),
                    timestamp: Utc::now(),
                    value: value.unwrap_or(0.0),
                    threshold: thr,
                    resolved: false,
                    resolved_at: None,
                };
                info!("alert fire: {:?} value={:?} thr={thr}", kind, value);
                state.alerts.push_back(a.clone());
                if state.alerts.len() > 128 {
                    state.alerts.pop_front();
                }
                new_alerts.push(a);
            }
        }
        None if pre_existing => {
            let c = counters.entry(key.clone()).or_insert(0);
            *c += 1;
            info!(
                "alert hysteresis tick: kind={:?} value={:?} warn={} crit={} counter={}/{}",
                kind, value, warn_t, crit_t, *c, CLEAR_TICKS_REQUIRED
            );
            if *c >= CLEAR_TICKS_REQUIRED {
                for a in state.alerts.iter_mut() {
                    if a.kind == kind && a.source_id == source_id && !a.resolved {
                        a.resolved = true;
                        a.resolved_at = Some(Utc::now());
                        info!("alert resolve: {:?}", kind);
                    }
                }
                counters.remove(&key);
            }
        }
        _ => {
            counters.remove(&key);
        }
    }
}
