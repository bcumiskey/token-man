pub mod admin;
pub mod ccusage;
pub mod otel;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::state::{SourceKind, SourceStatus};

/// Events pushed onto the aggregator channel by source workers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceEvent {
    Usage(UsageEvent),
    SessionStart {
        source_id: String,
        kind: SourceKind,
        project: Option<String>,
        model: Option<String>,
    },
    StatusChange {
        source_id: String,
        status: SourceStatus,
    },
    /// Admin API aggregate snapshot for opaque sources.
    AdminSnapshot {
        kind: SourceKind,
        owner: String,
        cost_today: f32,
        tokens_today: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub source_id: String,
    pub kind: SourceKind,
    pub owner: String,
    pub timestamp: DateTime<Utc>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub project: Option<String>,
    pub context_tokens: Option<u64>, // running total for context % if known
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub is_sidechain: Option<bool>,
    #[serde(default)]
    pub turn_type: Option<String>,
}
