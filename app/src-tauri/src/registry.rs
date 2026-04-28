use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRegistry {
    pub version: String,
    pub updated: String,
    #[serde(default)]
    pub models: HashMap<String, ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub context_window: u64,
    #[serde(default = "one")]
    pub tokenizer_inflation: f32,
    pub pricing_input_per_mtok: f32,
    pub pricing_output_per_mtok: f32,
    #[serde(default = "ten_percent")]
    pub cache_read_multiplier: f32,
    #[serde(default = "one_twenty_five")]
    pub cache_write_multiplier: f32,
}

fn one() -> f32 { 1.0 }
fn ten_percent() -> f32 { 0.1 }
fn one_twenty_five() -> f32 { 1.25 }

impl ModelRegistry {
    pub fn load_bundled() -> Result<Self> {
        let bytes = include_bytes!("../assets/model-registry.json");
        let r: ModelRegistry = serde_json::from_slice(bytes)?;
        Ok(r)
    }

    /// Cost for a single turn, in USD.
    pub fn cost(&self, model: &str, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f32 {
        let Some(m) = self.models.get(model) else {
            return 0.0;
        };
        let per = 1_000_000.0_f32;
        let input_cost = (input as f32 / per) * m.pricing_input_per_mtok;
        let output_cost = (output as f32 / per) * m.pricing_output_per_mtok;
        let read_cost = (cache_read as f32 / per) * m.pricing_input_per_mtok * m.cache_read_multiplier;
        let write_cost = (cache_write as f32 / per) * m.pricing_input_per_mtok * m.cache_write_multiplier;
        input_cost + output_cost + read_cost + write_cost
    }

    pub fn context_window(&self, model: &str) -> Option<u64> {
        self.models.get(model).map(|m| m.context_window)
    }

    /// Tokenizer inflation factor — multiply displayed token counts by this to
    /// estimate true context-window consumption. Returns 1.0 when unknown.
    pub fn tokenizer_inflation(&self, model: &str) -> f32 {
        self.models.get(model).map(|m| m.tokenizer_inflation).unwrap_or(1.0)
    }
}
