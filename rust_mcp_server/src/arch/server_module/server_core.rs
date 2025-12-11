use extrema_infra::prelude::*;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::server_base::McpServer;

impl Strategy for McpServer {
    async fn initialize(&mut self) {
        if let Err(e) = self.model_data_init() {
            error!("Failed to init model data: {:?}", e);
        }
        info!("McpServer initialized");
    }
}
impl CommandEmitter for McpServer {
    fn command_init(&mut self, command_handle: Arc<CommandHandle>) {
        self.command_handles.push(command_handle);
    }

    fn command_registry(&self) -> Vec<Arc<CommandHandle>> {
        self.command_handles.clone()
    }
}

impl EventHandler for McpServer {
    async fn on_schedule(&mut self, msg: InfraMsg<AltScheduleEvent>) {
        if let Err(e) = self.periodic_send_data_to_model().await {
            warn!("Failed to send data: {:?}, task: {:?}", e, msg.task_id);
        }
    }
    
    async fn on_preds(&mut self, msg: InfraMsg<AltTensor>) {
        if let Err(e) = self.mcp_mediator(&msg.data).await {
            warn!("Failed to process MCP Mediator: {:?}, task: {:?}", e, msg.task_id);
        }
    }

    async fn on_ws_event(&mut self, msg: InfraMsg<WsTaskInfo>) {
        if !matches!(msg.data.ws_channel, WsChannel::Trades(..)) {
            return;
        }

        if let Err(e) = self.connect_channel(&msg.data.ws_channel).await {
            error!("Failed to connect binance trade channel: {:?}", e);
        }
    }

    async fn on_trade(&mut self, msg: InfraMsg<Vec<WsTrade>>) {
        for t in msg.data.iter() {
            self.px.insert(t.inst.to_string(), t.price);
        }
    }
}
