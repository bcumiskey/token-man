//! Token-Man — Winamp-inspired HUD for Claude token usage.
//!
//! Single Tauri process. Backend spawns source workers that push events onto an
//! mpsc channel; the aggregator consumes them, applies thresholds, and emits a
//! `state-update` IPC event on each render tick.

mod aggregator;
mod audio;
mod config;
mod db;
mod ipc;
mod paths;
mod registry;
mod snapshot;
mod sources;
mod state;
mod thresholds;
mod tray;

use std::sync::Arc;

use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

use crate::aggregator::Aggregator;
use crate::config::Config;
use crate::db::Db;
use crate::sources::SourceEvent;
use crate::state::AppState;

pub struct AppContext {
    pub state: Arc<RwLock<AppState>>,
    pub db: Arc<Db>,
    pub config: Arc<RwLock<Config>>,
    pub event_tx: mpsc::Sender<SourceEvent>,
    pub command_tx: mpsc::Sender<ipc::BackendCommand>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("TOKEN_MAN_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Token-Man starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            // Load config + open DB.
            let config_path = paths::config_path(&handle);
            let config = Config::load_or_default(&config_path)?;
            let db_path = paths::db_path(&handle);
            let db = Arc::new(Db::open(&db_path)?);

            let state = Arc::new(RwLock::new(AppState::new(&config, &db)?));
            let config = Arc::new(RwLock::new(config));

            let (event_tx, event_rx) = mpsc::channel::<SourceEvent>(1024);
            let (command_tx, command_rx) = mpsc::channel::<ipc::BackendCommand>(64);

            let ctx = Arc::new(AppContext {
                state: state.clone(),
                db: db.clone(),
                config: config.clone(),
                event_tx: event_tx.clone(),
                command_tx,
            });
            app.manage(ctx.clone());

            // Tray.
            if let Err(e) = tray::install(&handle) {
                error!("tray install failed: {e:?}");
            }

            // Spawn workers.
            let aggregator = Aggregator::new(state.clone(), db.clone(), config.clone(), handle.clone());
            tauri::async_runtime::spawn(async move {
                aggregator.run(event_rx, command_rx).await;
            });

            let ev_tx = event_tx.clone();
            let cfg = config.clone();
            let ccu_db = db.clone();
            tauri::async_runtime::spawn(async move {
                sources::ccusage::run(ev_tx, cfg, ccu_db).await;
            });

            let ev_tx = event_tx.clone();
            let cfg = config.clone();
            tauri::async_runtime::spawn(async move {
                sources::otel::run(ev_tx, cfg).await;
            });

            let ev_tx = event_tx.clone();
            let cfg = config.clone();
            tauri::async_runtime::spawn(async move {
                sources::admin::run(ev_tx, cfg).await;
            });

            // Render tick — reads state + emits to frontend.
            let render_state = state.clone();
            let render_cfg = config.clone();
            let render_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let interval_ms = render_cfg.read().await.app.render_interval_ms.max(100);
                    tokio::time::sleep(std::time::Duration::from_millis(interval_ms as u64)).await;
                    let view = render_state.read().await.build_view();
                    if let Err(e) = render_handle.emit("state-update", &view) {
                        error!("emit state-update failed: {e:?}");
                    }
                }
            });

            // Apply pin default from config before first show so users don't
            // see a flicker where the window isn't pinned despite the toggle
            // showing active.
            if let Some(w) = handle.get_webview_window("main") {
                let pinned = config
                    .try_read()
                    .map(|c| c.app.always_on_top_default)
                    .unwrap_or(true);
                let _ = w.set_always_on_top(pinned);
            }

            // Play jingle + show splash after first tick.
            let jingle_handle = handle.clone();
            let jingle_cfg = config.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                if jingle_cfg.read().await.app.jingle_on_launch {
                    audio::play_jingle_async().await;
                }
                if let Some(w) = jingle_handle.get_webview_window("main") {
                    let _ = w.show();
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::cmd_toggle_pin,
            ipc::cmd_minimize,
            ipc::cmd_close,
            ipc::cmd_snap_to_clipboard,
            ipc::cmd_set_spectrum_window,
            ipc::cmd_select_session,
            ipc::cmd_refresh_registry,
            ipc::cmd_open_settings,
            ipc::cmd_set_profile,
            ipc::cmd_get_config,
            ipc::cmd_save_config,
            ipc::cmd_set_admin_key,
            ipc::cmd_test_alert,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
