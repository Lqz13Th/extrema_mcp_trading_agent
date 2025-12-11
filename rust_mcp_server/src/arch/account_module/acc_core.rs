use reqwest::Client;
use std::sync::Arc;
use tracing::{error, info, warn};

use extrema_infra::prelude::*;

use super::acc_base::AccountManager;
impl Strategy for AccountManager {
    async fn initialize(&mut self) {
        let shared_client = Arc::new(Client::new());
        if let Err(e) = self.load_all_accounts(shared_client) {
            error!("Failed to init account manager: {:?}", e);
        }

        if let Err(e) = self.init_inst_info().await {
            error!("Init instrument info failed: {:?}", e);
        }

        if let Err(e) = self.update_accounts().await {
            error!("Init accounts info failed: {:?}", e);
        }

        info!("Account manager initialized");
    }
}

impl CommandEmitter for AccountManager {
    fn command_init(&mut self, command_handle: Arc<CommandHandle>) {
        self.command_handles.push(command_handle);
    }

    fn command_registry(&self) -> Vec<Arc<CommandHandle>> {
        self.command_handles.clone()
    }
}

impl EventHandler for AccountManager {
    async fn on_schedule(&mut self, msg: InfraMsg<AltScheduleEvent>) {
        match msg.task_id {
            id if id == self.config.reload_task_id => {
                if let Err(e) = self.reload_accounts().await {
                    error!("Reload accounts failed: {:?}", e);
                }
            },
            id if id == self.config.update_task_id => {
                if let Err(e) = self.update_accounts().await {
                    error!("Update accounts failed: {:?}", e);
                }

                if let Err(e) = self.process_weights().await {
                    warn!(
                        "Failed to process weights: {:?}, task: {:?}",
                        e, msg.task_id
                    );
                }
            },
            _ => {},
        };
    }

    async fn on_preds(&mut self, msg: InfraMsg<AltTensor>) {
        if let Err(e) = self.process_weights().await {
            warn!(
                "Failed to process weights: {:?}, task: {:?}",
                e, msg.task_id
            );
        }
    }

    async fn on_ws_event(&mut self, msg: InfraMsg<WsTaskInfo>) {
        if let Err(e) = self.process_ws_event(&msg).await {
            error!("Failed to process ws account event: {:?}", e);
        }
    }

    async fn on_acc_order(&mut self, msg: InfraMsg<Vec<WsAccOrder>>) {
        self.process_acc_order(&msg);
    }

    async fn on_acc_bal_pos(&mut self, msg: InfraMsg<Vec<WsAccBalPos>>) {
        self.process_bal_pos(&msg);
    }
}
