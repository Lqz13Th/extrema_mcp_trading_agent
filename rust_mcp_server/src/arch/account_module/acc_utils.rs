use extrema_infra::{
    arch::market_assets::{api_data::utils_data::InstrumentInfo, api_general::normalize_to_string},
    errors::{InfraError, InfraResult},
};
use serde::Deserialize;
use std::{env::current_dir, fs};
use tracing::{error, info};

#[derive(Clone, Debug, Deserialize)]
pub struct AccountFileConfig {
    pub account_id: String,
    pub exchange: String,
    pub api_key: String,
    pub api_secret: String,
    pub passphrase: Option<String>,
    pub account_orders_task_id: u64,
    pub account_bal_pos_task_id: u64,
}

pub fn load_account_config() -> InfraResult<Vec<AccountFileConfig>> {
    let mut path = current_dir().map_err(|e| {
        InfraError::Msg(format!(
            "Failed to get current directory for account config: {}",
            e,
        ))
    })?;

    path.push("account_config.json");

    info!("account_config path: {:?}", path);

    if !path.exists() {
        error!("account_config.json not found at {:?}", path);
        return Err(InfraError::EnvVarMissing(
            "account config path does not exist".into(),
        ));
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| InfraError::Msg(format!("Failed to read account config file: {}", e)))?;

    let configs: Vec<AccountFileConfig> = serde_json::from_str(&content)
        .map_err(|e| InfraError::Msg(format!("Failed to parse account config: {}", e)))?;

    Ok(configs)
}

#[derive(Clone, Debug)]
pub struct AccountInitConfig {
    pub reload_task_id: u64,
    pub update_task_id: u64,
    pub reload_interval_sec: u64,
    pub update_interval_sec: u64,
}

impl Default for AccountInitConfig {
    fn default() -> Self {
        Self {
            reload_task_id: 10,
            update_task_id: 20,
            reload_interval_sec: 3600,
            update_interval_sec: 30,
        }
    }
}

pub fn calc_okx_order_size(
    price: f64,
    notional: f64,
    info: &InstrumentInfo,
) -> InfraResult<String> {
    let ct_val = info
        .contract_value
        .ok_or_else(|| InfraError::Msg("okx contract_value missing".into()))?;

    let mut size = notional / (price * ct_val);
    let min_sz = info.min_lmt_size.max(info.min_mkt_size);
    let max_sz = info.max_lmt_size.min(info.max_mkt_size);
    size = size.clamp(min_sz, max_sz);

    Ok(normalize_to_string(size, info.lot_size))
}

pub fn calc_binance_order_size(
    price: f64,
    notional: f64,
    info: &InstrumentInfo,
) -> InfraResult<String> {
    let mut size = notional / price;
    let min_sz = info.min_lmt_size.max(info.min_mkt_size);
    let max_sz = info.max_lmt_size.min(info.max_mkt_size);
    size = size.clamp(min_sz, max_sz);
    println!("size: {}", size);
    println!("price: {}", price);
    println!("notional: {}", notional);
    println!("min_sz: {}", min_sz);
    println!("max_sz: {}", max_sz);
    println!("lot_size: {}", info.lot_size);
    Ok(normalize_to_string(size, info.lot_size))
}
