use std::path::PathBuf;

use tauri::{AppHandle, Manager};

pub fn app_data_dir(handle: &AppHandle) -> PathBuf {
    handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("Token-Man"))
}

pub fn config_path(handle: &AppHandle) -> PathBuf {
    let d = app_data_dir(handle);
    std::fs::create_dir_all(&d).ok();
    d.join("config.toml")
}

pub fn db_path(handle: &AppHandle) -> PathBuf {
    let d = app_data_dir(handle);
    std::fs::create_dir_all(&d).ok();
    d.join("state.db")
}

/// Default ccusage JSONL roots. Claude Code writes to `~/.claude/projects/`.
pub fn default_ccusage_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".claude").join("projects"));
    }
    out
}
