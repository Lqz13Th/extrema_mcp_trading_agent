use dashmap::DashMap;
use std::{sync::Arc, time::Duration};
use tracing::info;
use tracing_subscriber;

use extrema_infra::prelude::*;


mod arch;
use arch::{
    account_module::{
        acc_base::{AccountManager, TargetWeights},
        acc_utils::AccountInitConfig,
    },
    server_module::server_base::McpServer,
};

fn build_account_ws_tasks() -> Vec<TaskInfo> {
    vec![
        TaskInfo::WsTask(Arc::new(WsTaskInfo {
            market: Market::Okx,
            ws_channel: WsChannel::AccountOrders,
            filter_channels: false,
            chunk: 1,
            task_base_id: Some(1100),
        })),
        TaskInfo::WsTask(Arc::new(WsTaskInfo {
            market: Market::Okx,
            ws_channel: WsChannel::AccountBalAndPos,
            filter_channels: false,
            chunk: 1,
            task_base_id: Some(1150),
        })),
    ]
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("Logger initialized");

    let shared_inst_target_weight: TargetWeights = Arc::new(DashMap::new());

    let acc_config = AccountInitConfig {
        reload_task_id: 2,
        update_task_id: 3,
        reload_interval_sec: 3600,
        update_interval_sec: 60,
    };

    // Machine Learning models
    let model_task = AltTaskInfo {
        alt_task_type: AltTaskType::ModelPreds(5001), // Zeromq port
        chunk: 1,
        task_base_id: Some(5001), // Custom task ID
    };

    let alt_data_scheduler_task = AltTaskInfo {
        alt_task_type: AltTaskType::TimeScheduler(Duration::from_secs(180)),
        chunk: 1,
        task_base_id: None,
    };

    // For periodic reload account info from config
    let acc_reload_scheduler_task = AltTaskInfo {
        alt_task_type: AltTaskType::TimeScheduler(Duration::from_secs(
            acc_config.reload_interval_sec,
        )),
        chunk: 1,
        task_base_id: Some(acc_config.reload_task_id),
    };

    // Update account Pos & Bal info
    let acc_update_scheduler_task = AltTaskInfo {
        alt_task_type: AltTaskType::TimeScheduler(Duration::from_secs(
            acc_config.update_interval_sec,
        )),
        chunk: 1,
        task_base_id: Some(acc_config.update_task_id),
    };

    let binance_ws_trade = WsTaskInfo {
        market: Market::BinanceUmFutures,
        ws_channel: WsChannel::Trades(None),
        filter_channels: false,
        chunk: 1,
        task_base_id: None,
    };

    let mut account_module = AccountManager::new(acc_config);
    let mut mcp_server = McpServer::new();

    account_module.with_target_weights(shared_inst_target_weight.clone());
    mcp_server.with_target_weights(shared_inst_target_weight.clone());
   
    let env = EnvBuilder::new()
        .with_board_cast_channel(BoardCastChannel::default_alt_event())
        .with_board_cast_channel(BoardCastChannel::default_ws_event())
        .with_board_cast_channel(BoardCastChannel::default_trade())
        .with_board_cast_channel(BoardCastChannel::default_scheduler())
        .with_board_cast_channel(BoardCastChannel::default_model_preds())
        .with_board_cast_channel(BoardCastChannel::default_account_order())
        .with_board_cast_channel(BoardCastChannel::default_account_bal_pos())
        .with_task(TaskInfo::AltTask(Arc::new(model_task)))
        .with_task(TaskInfo::AltTask(Arc::new(alt_data_scheduler_task)))
        .with_task(TaskInfo::AltTask(Arc::new(acc_reload_scheduler_task)))
        .with_task(TaskInfo::AltTask(Arc::new(acc_update_scheduler_task)))
        .with_task(TaskInfo::WsTask(Arc::new(binance_ws_trade)))
        .with_tasks(build_account_ws_tasks())
        .with_strategy_module(account_module)
        .with_strategy_module(mcp_server)
        .build();

    // Start event loop (spawns all tasks, connects strategies, begins message flow)
    env.execute().await;
}
