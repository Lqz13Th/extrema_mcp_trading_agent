use serde::Deserialize;
use std::{env::current_dir, fs};
use tracing::{error, info};

use extrema_infra::errors::*;

pub fn load_model_config() -> InfraResult<Vec<ModelConfig>> {
    let mut path = current_dir()?;
    path.push("model_config.json");

    info!("model_config path: {:?}", path);

    if !path.exists() {
        error!("model_config.json not found at {:?}", path);
        return Err(InfraError::EnvVarMissing(
            "model config path does not exist".into(),
        ));
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| InfraError::Msg(format!("Failed to read model config file: {}", e)))?;

    let configs: Vec<ModelConfig> = serde_json::from_str(&content)
        .map_err(|e| InfraError::Msg(format!("Failed to parse model config: {}", e)))?;

    Ok(configs)
}


#[derive(Clone, Debug, Deserialize)]
pub struct ModelConfig {
    pub port: u64,
    pub model_id: String,
    pub account_id: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            port: 0,
            model_id: "".to_string(),
            account_id: "".to_string(),
        }
    }
}
