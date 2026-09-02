//! Admin API poller for opaque sources (claude.ai, Cowork, Chrome, Excel, API).
//!
//! Per-profile polling on `admin_poll_interval_s`. The Admin API key is pulled
//! from the OS keychain. On failure, emits a StatusChange to mark the source
//! stale; auto-retries with exponential backoff.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::sources::SourceEvent;
use crate::state::{SourceKind, SourceStatus};

const KEYRING_SERVICE: &str = "token-man";

pub async fn run(tx: mpsc::Sender<SourceEvent>, cfg: Arc<RwLock<Config>>) {
    let mut backoff = Duration::from_secs(60);
    // F12 fix v1.0.6: rate-limit "admin poll failed" warns to once per
    // (profile, process). Repeated failures still trigger StatusChange
    // events and exponential backoff; the log was the spam.
    let mut warned_profiles: HashSet<String> = HashSet::new();

    loop {
        let (interval_s, profiles, endpoint) = {
            let c = cfg.read().await;
            (
                c.app.admin_poll_interval_s.max(30) as u64,
                c.profiles.clone(),
                c.sources
                    .admin_api_endpoint
                    .clone()
                    .unwrap_or_else(|| "https://api.anthropic.com/v1/organizations/usage".into()),
            )
        };

        let mut any_configured = false;
        for profile in &profiles {
            let Some(key_ref) = profile.admin_api_key_ref.as_deref() else {
                continue;
            };
            any_configured = true;
            let key = match load_admin_key(key_ref) {
                Ok(k) => k,
                Err(e) => {
                    debug!("no admin key for profile {}: {e:?}", profile.id);
                    continue;
                }
            };
            match poll_profile(&endpoint, &key, &profile.id).await {
                Ok(rows) => {
                    backoff = Duration::from_secs(60);
                    for row in rows {
                        let _ = tx.send(SourceEvent::AdminSnapshot {
                            kind: row.kind,
                            owner: profile.id.clone(),
                            cost_today: row.cost_today,
                            tokens_today: row.tokens_today,
                        }).await;
                    }
                }
                Err(e) => {
                    if warned_profiles.insert(profile.id.clone()) {
                        warn!(
                            "admin poll failed for profile {}: {e:?} (further failures suppressed for this session)",
                            profile.id
                        );
                    } else {
                        debug!("admin poll failed for profile {}: {e:?}", profile.id);
                    }
                    for k in [SourceKind::ClaudeAi, SourceKind::Cowork, SourceKind::Chrome, SourceKind::Excel, SourceKind::Api] {
                        let _ = tx.send(SourceEvent::StatusChange {
                            source_id: format!("{}:{}", k.label(), profile.id),
                            status: SourceStatus::Stale,
                        }).await;
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(1800));
                }
            }
        }

        let sleep = if any_configured {
            Duration::from_secs(interval_s)
        } else {
            Duration::from_secs(interval_s * 2)
        };
        tokio::time::sleep(sleep.max(backoff)).await;
    }
}

pub fn load_admin_key(key_ref: &str) -> anyhow::Result<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, key_ref)?;
    Ok(entry.get_password()?)
}

pub fn store_admin_key(key_ref: &str, value: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, key_ref)?;
    entry.set_password(value)?;
    Ok(())
}

struct Row {
    kind: SourceKind,
    cost_today: f32,
    tokens_today: u64,
}

#[derive(Debug, Deserialize)]
struct AdminResponse {
    #[serde(default)]
    data: Vec<AdminBucket>,
}

#[derive(Debug, Deserialize)]
struct AdminBucket {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    cost_usd: Option<f32>,
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

async fn poll_profile(endpoint: &str, key: &str, _profile_id: &str) -> anyhow::Result<Vec<Row>> {
    info!("admin poll -> {endpoint}");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let resp = client
        .get(endpoint)
        .query(&[("starting_at", today.as_str())])
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("admin api returned {}", resp.status());
    }
    let body: AdminResponse = resp.json().await?;
    let mut out = Vec::new();
    for b in body.data {
        let surface = b.surface.or(b.source).unwrap_or_default().to_lowercase();
        let kind = match surface.as_str() {
            "claude_code" | "code" => SourceKind::Code,
            "cowork" => SourceKind::Cowork,
            "chrome" => SourceKind::Chrome,
            "excel" => SourceKind::Excel,
            "claude_ai" | "claude.ai" | "chat" => SourceKind::ClaudeAi,
            _ => SourceKind::Api,
        };
        let tokens = b.input_tokens.unwrap_or(0) + b.output_tokens.unwrap_or(0);
        out.push(Row {
            kind,
            cost_today: b.cost_usd.unwrap_or(0.0),
            tokens_today: tokens,
        });
    }
    Ok(out)
}
