use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub app: AppSection,
    #[serde(default)]
    pub thresholds: Thresholds,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub sources: SourcesSection,
    #[serde(default)]
    pub registry: RegistrySection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSection {
    pub render_interval_ms: u32,
    pub admin_poll_interval_s: u32,
    pub minimize_to_tray: bool,
    pub autostart: bool,
    pub notification_sound: bool,
    pub notification_toasts: bool,
    pub always_on_top_default: bool,
    pub red_mode_enabled: bool,
    pub jingle_on_launch: bool,
    pub otel_port: u16,
    pub active_profile: String,
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            render_interval_ms: 500,
            admin_poll_interval_s: 60,
            minimize_to_tray: true,
            autostart: false,
            notification_sound: true,
            notification_toasts: true,
            always_on_top_default: true,
            red_mode_enabled: true,
            jingle_on_launch: true,
            otel_port: 4318,
            active_profile: "you".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub ctx_warn: f32,
    pub ctx_critical: f32,
    pub cache_warn: f32,
    pub cache_critical: f32,
    pub burn_warn: f32,
    pub burn_critical: f32,
    pub eta_warn_min: u32,
    pub eta_critical_min: u32,
    pub five_hour_warn: f32,
    pub five_hour_critical: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            ctx_warn: 70.0,
            ctx_critical: 85.0,
            cache_warn: 80.0,
            cache_critical: 70.0,
            burn_warn: 3.00,
            burn_critical: 5.00,
            eta_warn_min: 120,
            eta_critical_min: 30,
            five_hour_warn: 70.0,
            five_hour_critical: 85.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub admin_api_key_ref: Option<String>,
    #[serde(default = "default_plan")]
    pub plan_type: String,
    #[serde(default)]
    pub five_hour_limit: u64,
    #[serde(default)]
    pub weekly_limit: Option<u64>,
}

fn default_plan() -> String {
    "Pro".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourcesSection {
    #[serde(default)]
    pub ccusage_paths: Vec<PathBuf>,
    #[serde(default)]
    pub admin_api_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySection {
    #[serde(default = "default_registry_url")]
    pub update_url: String,
}

impl Default for RegistrySection {
    fn default() -> Self {
        Self {
            update_url: default_registry_url(),
        }
    }
}

fn default_registry_url() -> String {
    // Points at this repo's own bundled registry on `main`. The previous
    // default (tokenman/token-man) is not a repo that exists, so
    // cmd_refresh_registry had always failed with HTTP 404 - the self-update
    // path was dead from the first commit. This repo is public, so the raw
    // fetch needs no auth.
    "https://raw.githubusercontent.com/bcumiskey/token-man/main/app/src-tauri/assets/model-registry.json"
        .into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app: AppSection::default(),
            thresholds: Thresholds::default(),
            profiles: vec![Profile {
                id: "you".into(),
                name: "you".into(),
                color: "#378ADD".into(),
                admin_api_key_ref: None,
                plan_type: "Pro".into(),
                five_hour_limit: 200_000,
                weekly_limit: Some(5_000_000),
            }],
            sources: SourcesSection::default(),
            registry: RegistrySection::default(),
        }
    }
}

impl Config {
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            let cfg = Self::default();
            cfg.save(path)?;
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(path)?;
        match toml::from_str::<Config>(&raw) {
            Ok(c) => Ok(c),
            Err(e) => {
                tracing::warn!("config parse failed ({e}); using defaults + keeping old file");
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)?;
        std::fs::write(path, s)?;
        Ok(())
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|p| p.id == self.app.active_profile)
            .or_else(|| self.profiles.first())
    }
}
