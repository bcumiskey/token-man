use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::AppContext;
use crate::sources::admin;
use crate::state::SpectrumWindow;

#[derive(Debug, Clone)]
pub enum BackendCommand {
    SetSpectrumWindow(SpectrumWindow),
    SelectSession(String),
    TestAlert,
}

#[tauri::command]
pub async fn cmd_toggle_pin(app: AppHandle, ctx: State<'_, Arc<AppContext>>) -> Result<bool, String> {
    let win = app.get_webview_window("main").ok_or("no main window")?;
    let current = win.is_always_on_top().map_err(|e| e.to_string())?;
    let next = !current;
    win.set_always_on_top(next).map_err(|e| e.to_string())?;
    ctx.state.write().await.pin_active = next;
    Ok(next)
}

#[tauri::command]
pub async fn cmd_minimize(app: AppHandle) -> Result<(), String> {
    let win = app.get_webview_window("main").ok_or("no main window")?;
    let current = win.inner_size().map_err(|e| e.to_string())?;
    let _full = current.height > 60;
    // Toggle between full and mini sizes.
    let target_h = if _full { 32u32 } else { 280u32 };
    win.set_size(tauri::LogicalSize::new(540.0, target_h as f64))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_close(app: AppHandle, ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    let minimize_to_tray = ctx.config.read().await.app.minimize_to_tray;
    if let Some(win) = app.get_webview_window("main") {
        if minimize_to_tray {
            win.hide().map_err(|e| e.to_string())?;
        } else {
            win.close().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_snap_to_clipboard(app: AppHandle, ctx: State<'_, Arc<AppContext>>) -> Result<String, String> {
    let md = {
        let s = ctx.state.read().await;
        crate::snapshot::build_markdown(&s)
    };
    app.clipboard()
        .write_text(md.clone())
        .map_err(|e| e.to_string())?;
    Ok(md)
}

#[tauri::command]
pub async fn cmd_set_spectrum_window(
    ctx: State<'_, Arc<AppContext>>,
    window: String,
) -> Result<(), String> {
    let w = match window.as_str() {
        "60s" => SpectrumWindow::S60,
        "1h" => SpectrumWindow::H1,
        "24h" => SpectrumWindow::H24,
        _ => return Err("invalid window".into()),
    };
    ctx.command_tx
        .send(BackendCommand::SetSpectrumWindow(w))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_select_session(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<(), String> {
    ctx.command_tx
        .send(BackendCommand::SelectSession(source_id))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_refresh_registry(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
) -> Result<RegistryUpdate, String> {
    let url = ctx.config.read().await.registry.update_url.clone();
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("registry fetch: HTTP {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let reg: crate::registry::ModelRegistry =
        serde_json::from_str(&text).map_err(|e| format!("parse: {e}"))?;
    ctx.state.write().await.registry = reg.clone();
    let _ = app.emit_to(tauri::EventTarget::any(), "registry-updated", &reg);
    Ok(RegistryUpdate {
        version: reg.version,
        updated: reg.updated,
    })
}

#[derive(Debug, Serialize)]
pub struct RegistryUpdate {
    pub version: String,
    pub updated: String,
}

#[tauri::command]
pub async fn cmd_open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let _builder = tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("Token-Man — Settings")
    .inner_size(560.0, 560.0)
    .resizable(true)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn cmd_set_profile(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
    profile_id: String,
) -> Result<(), String> {
    let mut c = ctx.config.write().await;
    c.app.active_profile = profile_id.clone();
    let path = crate::paths::config_path(&app);
    c.save(&path).map_err(|e| e.to_string())?;
    drop(c);
    ctx.state.write().await.active_profile = profile_id;
    Ok(())
}

#[tauri::command]
pub async fn cmd_get_config(ctx: State<'_, Arc<AppContext>>) -> Result<crate::config::Config, String> {
    Ok(ctx.config.read().await.clone())
}

#[tauri::command]
pub async fn cmd_save_config(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
    config: crate::config::Config,
) -> Result<(), String> {
    let path = crate::paths::config_path(&app);
    config.save(&path).map_err(|e| e.to_string())?;
    // Mirror profile + threshold fields into AppState so the render tick can
    // reflect them without reaching into the config lock.
    {
        let mut s = ctx.state.write().await;
        s.profile_meta = config
            .profiles
            .iter()
            .map(|p| (p.id.clone(), (p.name.clone(), p.color.clone())))
            .collect();
        s.cache_warn_threshold = config.thresholds.cache_warn;
        s.cache_critical_threshold = config.thresholds.cache_critical;
    }
    *ctx.config.write().await = config;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct AdminKeyArgs {
    pub key_ref: String,
    pub value: String,
}

#[tauri::command]
pub async fn cmd_set_admin_key(args: AdminKeyArgs) -> Result<(), String> {
    admin::store_admin_key(&args.key_ref, &args.value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_test_alert(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.command_tx
        .send(BackendCommand::TestAlert)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Re-export for lib.rs usage.
use tauri::Emitter;
