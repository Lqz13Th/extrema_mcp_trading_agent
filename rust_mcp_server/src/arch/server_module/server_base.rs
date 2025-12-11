use std::collections::HashMap;
use std::sync::Arc;
use dashmap::DashMap;
use tracing::{error, info, warn};
use polars::prelude::*;


use extrema_infra::{
    prelude::*,
    arch::market_assets::{
        exchange::prelude::*,
        api_data::utils_data::OpenInterest,
    },
};
use extrema_infra::arch::market_assets::api_general::get_micros_timestamp;
use tokio::sync::oneshot;
use crate::arch::{
    account_module::acc_base::TargetWeights,
    feats::{
        alt_df_build::oi_to_lf,
        expr_operators::*,
    },
};
use super::{server_utils::{ModelConfig, load_model_config}};

#[derive(Clone, Debug)]
pub struct McpServer {
    binance_cm_cli: BinanceCmCli,
    okx_cli: OkxCli,
    pub px: HashMap<String, f64>,
    pub model_config: HashMap<String, ModelConfig>,
    pub target_weights: TargetWeights,
    pub command_handles: Vec<Arc<CommandHandle>>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            px: HashMap::new(),
            binance_cm_cli: BinanceCmCli::default(),
            okx_cli: OkxCli::default(),
            model_config: HashMap::new(),
            target_weights: Arc::new(DashMap::default()),
            command_handles: Vec::new(),
        }
    }

    pub fn with_target_weights(&mut self, target_weights: TargetWeights) -> &mut Self {
        self.target_weights = target_weights;
        self
    }

    pub fn model_data_init(&mut self) -> InfraResult<()> {
        info!("Starting model data initialization...");

        let configs = load_model_config()
            .map_err(|e| InfraError::Msg(format!("Failed to load model config: {}", e)))?;

        for cfg in configs {
            info!(
                "Initialized model: ModelID={} AccountID={}, Port={}",
                cfg.model_id,
                cfg.account_id,
                cfg.port,
            );

            self.model_config.insert(cfg.model_id.clone(), cfg);
        }

        Ok(())
    }

    pub async fn mcp_mediator(&mut self, alt_tensor: &AltTensor) -> InfraResult<()> {
        check_alt_tensor_error(alt_tensor)?;
        let cmd = alt_tensor
            .metadata
            .get("cmd")
            .map(|x| x.as_str())
            .unwrap_or("noop");

        match cmd {
            "adjust_position" => {
                let inst = alt_tensor
                    .metadata
                    .get("inst")
                    .cloned()
                    .unwrap_or_else(|| "DOGE_USDT_PERP".to_string());

                let new_target = alt_tensor
                    .metadata
                    .get("target_position")
                    .or_else(|| alt_tensor.metadata.get("pos_weight"))
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);

                let px_val = *self.px.entry(inst.clone()).or_insert(0.0);

                let old = self
                    .target_weights
                    .get(&inst)
                    .map(|v| *v)
                    .unwrap_or((px_val, 0.0));

                let new = (px_val, new_target);

                self.target_weights.insert(inst.clone(), new);

                info!(
                    "MCP adjust_position: inst={}, old={:?}, new={:?}",
                    inst, old, new
                );
            },
            "risk_alert" => {
                todo!()
            },
            "fallback" => {
                todo!()
            },
            "query" => {
                todo!()
            },
            "noop" => {
                info!("MCP mediator: noop for timestamp={}", alt_tensor.timestamp);
            },
            unknown => {
                warn!("Unknown MCP command: {}", unknown);
            },
        };

        Ok(())
    }

    pub async fn periodic_send_data_to_model(&mut self) -> InfraResult<()> {
        let oi_data = self.fetch_oi().await?;
        let df = self.process_oi(oi_data)?;
        self.send_data_to_model(&df).await?;

        Ok(())
    }

    async fn fetch_oi(&mut self) -> InfraResult<Vec<OpenInterest>> {
        let oi = self.binance_cm_cli.get_open_interest_history(
            "DOGE_USDT_PERP",
            "5m",
            InstrumentType::Perpetual,
            None,
            None,
            None,
        ).await?;

        Ok(oi)
    }

    fn process_oi(&mut self, oi_data: Vec<OpenInterest>) -> InfraResult<DataFrame> {
        let oi_lf = oi_to_lf(oi_data)
            .map_err(|e| InfraError::Msg(format!("Polars oi_to_lf err: {:?}", e)))?;

        let converted_oi_lf = convert_all_to_float64_except_timestamp(oi_lf)?;

        let schema = collect_schema_safe(&converted_oi_lf)?;
        let mut zscore_exprs = Vec::new();

        let exclude_cols = vec![
            "timestamp",
            "funding_funding_interval_hours",
            "funding_last_funding_rate",
            "premium_funding_spread",
            "adjusted_funding_rate",
            "funding_premium",
            "premium_open",
        ];

        for field in schema.iter_fields() {
            let name = field.name();
            let dtype = field.dtype();

            if exclude_cols.contains(&name.as_str()) {
                continue;
            }

            if *dtype == DataType::Float64 {
                zscore_exprs.push(z_score_expr(name, 20));
            }
        }

        let z_score_oi_df = converted_oi_lf
            .with_columns(zscore_exprs)
            .drop_nulls(None)
            .collect()?;

        Ok(z_score_oi_df)
    }

    async fn send_data_to_model(&self, data: &DataFrame) -> InfraResult<()> {
        for (model_id, _cfg) in &self.model_config {
            let inst = "DOGE_USDT_PERP".to_string();
            // 如果价格不存在，使用默认值 0.0（价格会在收到 trade 数据后更新）
            let px = self.px.get(&inst).copied().unwrap_or(0.0);
            
            if px == 0.0 {
                warn!("Price for {} not available yet, using 0.0. Waiting for trade data...", inst);
                // 可以选择跳过这次发送，等待价格数据
                continue;
            }

            let ts = get_micros_timestamp();
            let port = 5001;

            let pos_weight = self
                .target_weights
                .get(&inst)
                .map(|v| v.1)
                .unwrap_or(0.0);

            let tensor = df_to_tensor(
                data,
                model_id.clone(),
                px,
                pos_weight,
                ts,
            )?;

            println!("tensor: {:?}", tensor);

            if let Some(handle) = self.find_alt_handle(&AltTaskType::ModelPreds(port), port) {
                let cmd = TaskCommand::FeatInput(tensor);
                handle.send_command(cmd, None).await?;
            } else {
                error!("No model handle found for Model port: {}", port);
            }
        }

        Ok(())
    }

    pub(crate) async fn connect_channel(&self, channel: &WsChannel) -> InfraResult<()> {
        if let Some(handle) = self.find_ws_handle(channel, 1) {
            info!("Sending connect to {:?}", handle);

            // Step 1: Request connection URL
            let ws_url = self.okx_cli.get_public_connect_msg(channel).await?;
            let (tx, rx) = oneshot::channel();
            let cmd = TaskCommand::WsConnect {
                msg: ws_url,
                ack: AckHandle::new(tx),
            };
            handle.send_command(cmd, Some((AckStatus::WsConnect, rx))).await?;

            let insts = ["DOGE_USDT_PERP".to_string()];

            let ws_msg = self.okx_cli
                .get_public_sub_msg(channel, Some(&insts))
                .await?;

            let (tx, rx) = oneshot::channel();
            let cmd = TaskCommand::WsMessage {
                msg: ws_msg,
                ack: AckHandle::new(tx),
            };
            handle.send_command(cmd, Some((AckStatus::WsMessage, rx))).await?;
        } else {
            warn!(" No handle found for channel {:?}", channel);
        }

        Ok(())
    }
}

pub fn df_to_tensor(
    df: &DataFrame,
    model_id: String,
    price: f64,
    weight: f64,
    timestamp: u64,
) -> InfraResult<AltTensor> {
    if df.height() == 0 {
        return Err(InfraError::Msg("df is empty".into()));
    }

    let last_idx = df.height() - 1;

    let row = df
        .get_row(last_idx)
        .map_err(|_| InfraError::Msg("failed to get row".into()))?;

    let col_names: Vec<String> = df
        .get_columns()
        .iter()
        .map(|s| s.name().to_string())
        .collect();

    let mut data = Vec::with_capacity(row.0.len());
    for val in &row.0 {
        let f = match val {
            AnyValue::Float32(v) => *v,
            AnyValue::Float64(v) => *v as f32,
            AnyValue::Int64(v) => *v as f32,
            AnyValue::Int32(v) => *v as f32,
            AnyValue::UInt64(v) => *v as f32,
            AnyValue::UInt32(v) => *v as f32,
            _ => {
                return Err(InfraError::Msg(format!(
                    "unsupported type: {} ({:?})",
                    val,
                    val.dtype()
                )));
            }
        };
        data.push(f);
    }

    let shape = vec![data.len()];

    let mut metadata = HashMap::new();
    metadata.insert("model_id".to_string(), model_id);
    metadata.insert("price".to_string(), price.to_string());
    metadata.insert("pos_weight".to_string(), weight.to_string());
    metadata.insert("col_names".to_string(), serde_json::to_string(&col_names)?);

    Ok(AltTensor {
        timestamp,
        data,
        shape,
        metadata,
    })
}

pub fn check_alt_tensor_error(alt_tensor: &AltTensor) -> InfraResult<()> {
    if let Some(err_msg) = alt_tensor.metadata.get("error") {
        warn!(
            "Skipping AltTensor because Python side sent error: {} | timestamp={}",
            err_msg, alt_tensor.timestamp
        );
        return Err(InfraError::Msg(format!("Python model error: {}", err_msg)));
    }
    Ok(())
}