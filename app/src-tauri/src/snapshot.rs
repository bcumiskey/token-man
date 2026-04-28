use crate::state::{AppState, SourceStatus};

pub fn build_markdown(s: &AppState) -> String {
    let mut out = String::new();
    // Derive live count fresh from current sources so the snapshot matches
    // what the title/status bar show, rather than a potentially stale field.
    let live_now = s
        .sources
        .values()
        .filter(|src| matches!(src.status, SourceStatus::Active))
        .count();
    out.push_str("### Token-Man snapshot\n\n");
    out.push_str(&format!(
        "- Generated: {}\n",
        chrono::Utc::now().to_rfc3339()
    ));
    out.push_str(&format!(
        "- State: **{}** · profile: {} · {} live\n",
        s.global_state, s.active_profile, live_now
    ));
    out.push_str(&format!(
        "- Global: {:.0} tok/min · ${:.2} today · burn ${:.2}/hr · cache {:.0}%\n",
        s.global.tokens_per_min_current,
        s.global.cost_today,
        s.global.burn_rate_per_hour,
        s.global.cache_hit_rate_global
    ));
    out.push_str(&format!(
        "- Windows: ctx {:.0}% · 5h {:.0}% · week {:.0}% · eta {}\n\n",
        s.global.context_percent_worst,
        s.global.five_hour_usage_percent,
        s.global.week_usage_percent,
        s.global
            .limit_eta_min
            .map(|m| format!("{m}m"))
            .unwrap_or_else(|| "—".into())
    ));
    out.push_str("| source | model | project | ctx | t/m | cost | who | status |\n");
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    let mut sources: Vec<_> = s.sources.values().collect();
    sources.sort_by(|a, b| a.kind.label().cmp(b.kind.label()));
    for src in sources {
        let status = match src.status {
            SourceStatus::Active => "active",
            SourceStatus::Idle => "idle",
            SourceStatus::Stale => "stale",
            SourceStatus::Offline => "offline",
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | ${:.2} | {} | {} |\n",
            src.kind.label(),
            src.model,
            src.project.as_deref().unwrap_or("—"),
            src.context_percent.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".into()),
            src.tokens_per_min.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".into()),
            src.cost_today,
            src.owner,
            status,
        ));
    }
    out.push_str(&format!(
        "\n_registry v{} · {}_\n",
        s.registry.version, s.registry.updated
    ));
    out
}
