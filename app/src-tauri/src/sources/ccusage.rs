//! ccusage JSONL tailer.
//!
//! Watches `~/.claude/projects/**/*.jsonl`, reads newly appended lines, parses
//! them as Claude Code turn records, and forwards per-turn `UsageEvent`s.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[allow(unused_imports)]
use chrono::{DateTime, Utc};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::db::Db;
use crate::paths;
use crate::sources::{SourceEvent, UsageEvent};
use crate::state::SourceKind;

#[derive(Debug, Deserialize)]
struct JsonlLine {
    #[serde(default)]
    #[allow(dead_code)]
    uuid: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    message: Option<Message>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

pub async fn run(tx: mpsc::Sender<SourceEvent>, cfg: Arc<RwLock<Config>>, db: Arc<Db>) {
    let roots = {
        let c = cfg.read().await;
        if c.sources.ccusage_paths.is_empty() {
            paths::default_ccusage_roots()
        } else {
            c.sources.ccusage_paths.clone()
        }
    };

    info!("ccusage tailer starting with roots: {:?}", roots);

    // Map of file path -> byte offset already read.
    let mut offsets: HashMap<PathBuf, u64> = HashMap::new();

    // Seed from DB; for new or recently-modified files with no saved offset,
    // start at 0 so active sessions aren't truncated.
    let fresh_cutoff_ms = Utc::now().timestamp_millis() - 5 * 60 * 1000;
    for root in &roots {
        if !root.exists() {
            debug!("ccusage root missing: {}", root.display());
            continue;
        }
        for file in walk_jsonl(root) {
            let key = file.to_string_lossy().into_owned();
            let saved = db.get_tailer_offset(&key).unwrap_or(0);
            let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            let mtime_ms = std::fs::metadata(&file)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let start = if saved > 0 && saved <= size {
                saved
            } else if mtime_ms >= fresh_cutoff_ms {
                0
            } else {
                size
            };
            offsets.insert(file.clone(), start);
        }
    }

    // Setup filesystem watcher (channel to the poll loop).
    let (fs_tx, mut fs_rx) = mpsc::channel::<PathBuf>(256);
    let fs_tx_clone = fs_tx.clone();
    let mut watcher: RecommendedWatcher = match notify::recommended_watcher(
        move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                for p in ev.paths {
                    if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        let _ = fs_tx_clone.blocking_send(p);
                    }
                }
            }
        },
    ) {
        Ok(w) => w,
        Err(e) => {
            warn!("failed to create fs watcher: {e:?}; ccusage tailer disabled");
            return;
        }
    };

    for root in &roots {
        if root.exists() {
            if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
                warn!("watch failed for {}: {e:?}", root.display());
            }
        }
    }

    // Poll loop — also ticks every 2s as a safety net for missed events, and
    // flushes offsets to the DB every 60s.
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    let mut flush_tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            Some(path) = fs_rx.recv() => {
                read_appended(&path, &mut offsets, &tx).await;
            }
            _ = tick.tick() => {
                for root in &roots {
                    if !root.exists() { continue; }
                    for file in walk_jsonl(root) {
                        read_appended(&file, &mut offsets, &tx).await;
                    }
                }
            }
            _ = flush_tick.tick() => {
                for (path, off) in offsets.iter() {
                    let key = path.to_string_lossy().into_owned();
                    if let Err(e) = db.set_tailer_offset(&key, *off) {
                        debug!("set_tailer_offset failed for {key}: {e:?}");
                    }
                }
            }
        }
    }
}

fn walk_jsonl(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&p) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    out.push(path);
                }
            }
        }
    }
    out
}

async fn read_appended(
    path: &Path,
    offsets: &mut HashMap<PathBuf, u64>,
    tx: &mpsc::Sender<SourceEvent>,
) {
    let start = *offsets.get(path).unwrap_or(&0);
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(_) => return,
    };
    let size = meta.len();
    if size < start {
        // File was truncated or rotated; reset.
        offsets.insert(path.to_path_buf(), 0);
        return;
    }
    if size == start {
        return;
    }

    let mut reader = BufReader::new(file);
    if reader.seek(SeekFrom::Start(start)).is_err() {
        return;
    }

    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let source_id = format!("code:{}", session_id);

    let mut new_offset = start;
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = match reader.read_line(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        new_offset += n as u64;
        if buf.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<JsonlLine>(&buf) {
            Ok(line) => {
                if let Some(ev) = build_usage_event(&source_id, &line, path) {
                    let _ = tx.send(SourceEvent::Usage(ev)).await;
                }
            }
            Err(e) => {
                debug!("skip unparsable jsonl line in {}: {e}", path.display());
            }
        }
    }
    offsets.insert(path.to_path_buf(), new_offset);
}

fn build_usage_event(source_id: &str, line: &JsonlLine, path: &Path) -> Option<UsageEvent> {
    if line.kind.as_deref() != Some("assistant") {
        return None;
    }
    let msg = line.message.as_ref()?;
    let usage = msg.usage.as_ref()?;
    let ts: DateTime<Utc> = line
        .timestamp
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let model = msg.model.clone().unwrap_or_else(|| "unknown".into());
    let project = line.cwd.clone().map(|p| {
        Path::new(&p)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or(p)
    });
    Some(UsageEvent {
        source_id: source_id.to_string(),
        kind: SourceKind::Code,
        owner: "you".into(),
        timestamp: ts,
        model,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_input_tokens,
        cache_write_tokens: usage.cache_creation_input_tokens,
        project,
        context_tokens: Some(
            usage.input_tokens
                + usage.cache_read_input_tokens
                + usage.cache_creation_input_tokens,
        ),
        file_path: Some(path.to_string_lossy().into_owned()),
        is_sidechain: line.is_sidechain,
        turn_type: line.kind.clone(),
    })
}
