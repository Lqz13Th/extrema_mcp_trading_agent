use dashmap::DashMap;
use reqwest::Client;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::oneshot, time::sleep};
use tracing::{info, warn};

use extrema_infra::{
    arch::market_assets::{
        api_data::utils_data::InstrumentInfo, api_general::OrderParams, exchange::prelude::*,
    },
    prelude::*,
};

use super::acc_utils::*;

type InstKey = (String, Market);
pub type TargetWeights = Arc<DashMap<String, (f64, f64)>>;

#[derive(Clone, Debug)]
pub struct AccountManager {
    pub target_weights: TargetWeights,
    pub task_index: HashMap<u64, String>,
    pub account_infos: HashMap<String, AccountInfo>,
    pub instrument_infos: HashMap<InstKey, InstrumentInfo>,
    pub command_handles: Vec<Arc<CommandHandle>>,
    pub config: AccountInitConfig,
}

impl AccountManager {
    pub fn new(config: AccountInitConfig) -> Self {
        Self {
            target_weights: Arc::new(DashMap::new()),
            task_index: HashMap::new(),
            account_infos: HashMap::new(),
            instrument_infos: HashMap::new(),
            command_handles: Vec::new(),
            config,
        }
    }

    pub fn with_target_weights(&mut self, target_weights: TargetWeights) -> &mut Self {
        self.target_weights = target_weights;
        self
    }

    pub async fn init_inst_info(&mut self) -> InfraResult<()> {
        let okx_cli = OkxCli::default();
        let binance_cli = BinanceUmCli::default();

        let okx_inst_info = okx_cli
            .get_instrument_info(InstrumentType::Perpetual)
            .await?;
        let binance_inst_info = binance_cli
            .get_instrument_info(InstrumentType::Perpetual)
            .await?;

        self.insert_inst_info(Market::Okx, okx_inst_info);
        self.insert_inst_info(Market::BinanceUmFutures, binance_inst_info);

        Ok(())
    }

    fn insert_inst_info(&mut self, market: Market, infos: Vec<InstrumentInfo>) {
        for inst in infos {
            let key = (inst.inst.clone(), market.clone());
            self.instrument_infos.insert(key, inst);
        }
    }

    pub async fn process_weights(&mut self) -> InfraResult<()> {
        for account in self.account_infos.values_mut() {
            if let Err(e) = account
                .process_weight(&self.target_weights, &self.instrument_infos)
                .await
            {
                warn!(
                    "Failed to process account {}: {} — skipping",
                    account.account_id, e
                );
                continue;
            }
        }

        Ok(())
    }

    pub async fn process_ws_event(&self, msg: &InfraMsg<WsTaskInfo>) -> InfraResult<()> {
        let task_id = msg.task_id;

        let account_id = match self.task_index.get(&task_id) {
            Some(id) => id.clone(),
            None => return Ok(()),
        };

        let account = match self.account_infos.get(&account_id) {
            Some(acc) => acc,
            None => {
                warn!(
                    "[WS Event] task_id={} mapped to missing account_id={}",
                    task_id, account_id
                );
                return Ok(());
            },
        };

        info!(
            "[WS Event] Received message: account={} task_id={} channel={:?}",
            account.account_id, task_id, msg.data.ws_channel,
        );

        match &account.client {
            CexClients::BinanceUm(_) => {
                self.handle_binance_account_event(account, &msg.data.ws_channel)
                    .await?;
            },
            CexClients::Okx(_) => {
                self.handle_okx_account_event(account, &msg.data.ws_channel)
                    .await?;
            },
            _ => warn!(
                "[WS] Unsupported market for account={} task_id={} channel={:?}",
                account.account_id, task_id, msg.data.ws_channel,
            ),
        };

        Ok(())
    }

    pub fn process_acc_order(&mut self, msg: &InfraMsg<Vec<WsAccOrder>>) {
        let task_id = msg.task_id;

        let Some(account_id) = self.task_index.get(&task_id) else {
            warn!("[WS-Order] Unknown task_id={} — ignored", task_id);
            return;
        };

        let Some(account) = self.account_infos.get_mut(account_id) else {
            warn!(
                "[WS-Order] task_id={} mapped to account_id={}, but account missing",
                task_id, account_id
            );
            return;
        };

        for order in msg.data.iter() {
            let inst_key: InstKey = (order.inst.clone(), order.market.clone());
            if let Some(inst_info) = self.instrument_infos.get(&inst_key) {
                account.ws_update_acc_order(order, inst_info);
            }
        }
    }

    pub fn process_bal_pos(&mut self, msg: &InfraMsg<Vec<WsAccBalPos>>) {
        let task_id = msg.task_id;

        let Some(account_id) = self.task_index.get(&task_id) else {
            warn!("[WS-BP] Unknown task_id={} — ignored", task_id);
            return;
        };

        let Some(account) = self.account_infos.get_mut(account_id) else {
            warn!(
                "[WS-BP] task_id={} mapped to account_id={}, but account missing",
                task_id, account_id
            );
            return;
        };

        for bal_pos in msg.data.iter() {
            for pos in bal_pos.positions.iter() {
                let inst_key: InstKey = (pos.inst.clone(), bal_pos.market.clone());
                if let Some(inst_info) = self.instrument_infos.get(&inst_key) {
                    account.ws_update_acc_position(pos, inst_info);
                }
            }
        }
    }

    async fn handle_binance_account_event(
        &self,
        account: &AccountInfo,
        channel: &WsChannel,
    ) -> InfraResult<()> {
        let task_id = match channel {
            WsChannel::AccountOrders => account.account_orders_task_id,
            WsChannel::AccountBalAndPos => account.account_bal_pos_task_id,
            _ => {
                return Err(InfraError::Msg(format!(
                    "[WS] Unsupported WS channel for Binance: account={} channel={:?}",
                    account.account_id, channel,
                )));
            },
        };

        let Some(handle) = self.find_ws_handle(channel, task_id) else {
            warn!(
                "[WS] No WS handle found for Binance account={} channel={:?} task_id={}",
                account.account_id, channel, task_id,
            );
            return Ok(());
        };

        info!(
            "[WS Connect Start] Binance account={} channel={:?} task_id={}",
            account.account_id, channel, task_id,
        );
        let ws_url = account.client.get_private_connect_msg(channel).await?;
        let (tx, rx) = oneshot::channel();
        let cmd = TaskCommand::WsConnect {
            msg: ws_url,
            ack: AckHandle::new(tx),
        };
        handle
            .send_command(cmd, Some((AckStatus::WsConnect, rx)))
            .await?;

        info!(
            "[WS Done] Account={} channel={:?} task_id={} connected and subscribed",
            account.account_id, channel, task_id,
        );

        Ok(())
    }

    async fn handle_okx_account_event(
        &self,
        account: &AccountInfo,
        channel: &WsChannel,
    ) -> InfraResult<()> {
        let task_id = match channel {
            WsChannel::AccountOrders => account.account_orders_task_id,
            WsChannel::AccountBalAndPos => account.account_bal_pos_task_id,
            _ => {
                return Err(InfraError::Msg(format!(
                    "[WS] Unsupported WS channel for OKX: account={} channel={:?}",
                    account.account_id, channel,
                )));
            },
        };

        let Some(handle) = self.find_ws_handle(channel, task_id) else {
            warn!(
                "[WS] No WS handle found for OKX account={} channel={:?} task_id={}",
                account.account_id, channel, task_id,
            );
            return Ok(());
        };

        info!(
            "[WS Connect Start] OKX account={} channel={:?} task_id={}",
            account.account_id, channel, task_id,
        );

        // Step 1: Connect
        let ws_url = account.client.get_private_connect_msg(channel).await?;
        let (tx, rx) = oneshot::channel();
        let cmd = TaskCommand::WsConnect {
            msg: ws_url,
            ack: AckHandle::new(tx),
        };
        handle
            .send_command(cmd, Some((AckStatus::WsConnect, rx)))
            .await?;

        // Step 2: Login if needed
        let login_msg = match &account.client {
            CexClients::Okx(cli) => cli.ws_login_msg()?,
            e => {
                return Err(InfraError::Msg(format!(
                    "[WS] OKX account={} channel={:?} task_id={} login message creation failed: {:?}",
                    account.account_id, channel, task_id, e,
                )));
            },
        };
        let (tx, rx) = oneshot::channel();
        let cmd = TaskCommand::WsMessage {
            msg: login_msg,
            ack: AckHandle::new(tx),
        };
        handle
            .send_command(cmd, Some((AckStatus::WsMessage, rx)))
            .await?;
        sleep(Duration::from_millis(100)).await;

        // Step 3: Subscribe
        let sub_msg = account.client.get_private_sub_msg(channel).await?;
        let (tx, rx) = oneshot::channel();
        let cmd = TaskCommand::WsMessage {
            msg: sub_msg,
            ack: AckHandle::new(tx),
        };
        handle
            .send_command(cmd, Some((AckStatus::WsMessage, rx)))
            .await?;

        info!(
            "[WS Done] Account={} channel={:?} task_id={} connected and subscribed",
            account.account_id, channel, task_id
        );

        Ok(())
    }

    pub async fn update_accounts(&mut self) -> InfraResult<()> {
        for account in self.account_infos.values_mut() {
            if let Err(e) = account.rest_update_acc_balance().await {
                warn!(
                    "Failed to update balance for account {}: {} — skipping",
                    account.account_id, e,
                );

                continue;
            }

            if let Err(e) = account
                .rest_update_acc_pos_weight(&self.instrument_infos)
                .await
            {
                warn!(
                    "Failed to update position weights for account {}: {} — skipping",
                    account.account_id, e,
                );
                continue;
            }

            if let Err(e) = account
                .process_weight(&self.target_weights, &self.instrument_infos)
                .await
            {
                warn!(
                    "Failed to process account {}: {} — skipping",
                    account.account_id, e
                );
                continue;
            }
        }

        Ok(())
    }

    pub async fn reload_accounts(&mut self) -> InfraResult<()> {
        let new_cfgs = load_account_config()?;
        let shared_client = Arc::new(Client::new());

        let mut new_map = HashMap::new();
        for cfg in new_cfgs.iter() {
            let acc = AccountInfo::from_config(cfg, shared_client.clone())?;
            new_map.insert(cfg.account_id.clone(), acc);
        }

        let old_ids: HashSet<String> = self.account_infos.keys().cloned().collect();

        let new_ids: HashSet<String> = new_map.keys().cloned().collect();

        for acc_id in new_ids.difference(&old_ids) {
            if let Some(acc) = new_map.get(acc_id) {
                info!("[Account] New account detected: {}", acc_id);
                self.add_account(acc.clone());
                self.ws_connect_account(acc).await?;
            } else {
                warn!("[Account] new_ids contains unknown id: {}", acc_id);
            }
        }

        for acc_id in old_ids.difference(&new_ids) {
            info!("[Account] Account deleted from config: {}", acc_id);

            if let Some(old_acc) = self.account_infos.remove(acc_id) {
                self.task_index.remove(&old_acc.account_orders_task_id);
                self.task_index.remove(&old_acc.account_bal_pos_task_id);
                self.ws_disconnect_account(&old_acc).await?;
            }
        }

        for acc_id in new_ids.intersection(&old_ids) {
            let new_acc = match new_map.get(acc_id) {
                Some(a) => a.clone(),
                None => {
                    warn!("[Account] Failed to get new account: {}", acc_id);
                    continue;
                },
            };

            let old_acc = match self.account_infos.get(acc_id) {
                Some(a) => a.clone(),
                None => {
                    warn!(
                        "[Account] Internal map missing old account={} while updating",
                        acc_id
                    );
                    continue;
                },
            };

            if new_acc.config_changed(&old_acc) {
                info!("[Account] Account updated: {} (diff detected)", acc_id);

                self.account_infos.insert(acc_id.clone(), new_acc.clone());
                self.task_index.remove(&old_acc.account_orders_task_id);
                self.task_index.remove(&old_acc.account_bal_pos_task_id);

                self.task_index
                    .insert(new_acc.account_orders_task_id, acc_id.clone());
                self.task_index
                    .insert(new_acc.account_bal_pos_task_id, acc_id.clone());

                self.ws_disconnect_account(&old_acc).await?;
                self.ws_connect_account(&new_acc).await?;
            }
        }

        Ok(())
    }

    async fn ws_disconnect_account(&mut self, acc: &AccountInfo) -> InfraResult<()> {
        info!("[WS] Closing WS for account_id={}", acc.account_id);

        let close_list = [
            (WsChannel::AccountOrders, acc.account_orders_task_id),
            (WsChannel::AccountBalAndPos, acc.account_bal_pos_task_id),
        ];

        for (channel, task_id) in close_list {
            if let Some(handle) = self.find_ws_handle(&channel, task_id) {
                let (tx, _) = oneshot::channel();
                // need to finish shutdown logic
                let cmd = TaskCommand::WsShutdown {
                    msg: "".to_string(),
                    ack: AckHandle::new(tx),
                };
                handle.send_command(cmd, None).await?;
            } else {
                warn!(
                    "[WS] No handle found for channel={:?}, task_id={}",
                    channel, task_id
                );
            }
        }

        Ok(())
    }

    async fn ws_connect_account(&mut self, acc: &AccountInfo) -> InfraResult<()> {
        info!("[WS] Auto-connect for account_id={}", acc.account_id);

        match &acc.client {
            CexClients::BinanceUm(_) => {
                self.handle_binance_account_event(acc, &WsChannel::AccountOrders)
                    .await?;
                self.handle_binance_account_event(acc, &WsChannel::AccountBalAndPos)
                    .await?;
            },
            CexClients::Okx(_) => {
                self.handle_okx_account_event(acc, &WsChannel::AccountOrders)
                    .await?;
                self.handle_okx_account_event(acc, &WsChannel::AccountBalAndPos)
                    .await?;
            },
            _ => warn!("Unsupported exchange for auto connect: {:?}", acc.client),
        };

        Ok(())
    }

    pub fn load_all_accounts(&mut self, shared_client: Arc<Client>) -> InfraResult<()> {
        for cfg in load_account_config()? {
            let acc = AccountInfo::from_config(&cfg, shared_client.clone())?;
            self.add_account(acc);
        }
        Ok(())
    }

    fn add_account(&mut self, account_info: AccountInfo) {
        self.task_index.insert(
            account_info.account_orders_task_id,
            account_info.account_id.clone(),
        );

        self.task_index.insert(
            account_info.account_bal_pos_task_id,
            account_info.account_id.clone(),
        );

        self.account_infos
            .insert(account_info.account_id.clone(), account_info);
    }
}

#[derive(Clone, Debug)]
pub struct AccountInfo {
    pub account_id: String,
    pub client: CexClients,
    pub acc_weights: HashMap<String, f64>,
    pub inst_mark_price: HashMap<String, f64>,
    pub total_equity: f64,
    pub account_orders_task_id: u64,
    pub account_bal_pos_task_id: u64,
}

impl AccountInfo {
    fn ws_update_acc_order(&mut self, acc_order: &WsAccOrder, _inst_info: &InstrumentInfo) {
        info!("[Account] Update acc_order={:?}", acc_order);
    }

    fn ws_update_acc_position(&mut self, pos: &WsAccPosition, inst_info: &InstrumentInfo) {
        let mark_price = self
            .inst_mark_price
            .get(&pos.inst)
            .unwrap_or(&pos.avg_price);

        let pos_notional = match &self.client {
            CexClients::BinanceUm(_) => pos.size * mark_price,
            CexClients::Okx(_) => {
                let multiplier = inst_info.contract_value.unwrap_or(1.0);
                pos.size * mark_price * multiplier
            },
            _ => 0.0,
        };

        let weight = if self.total_equity > f64::EPSILON {
            pos_notional / self.total_equity
        } else {
            0.0
        };
        self.acc_weights.insert(pos.inst.clone(), weight);
    }

    pub async fn rest_update_acc_balance(&mut self) -> InfraResult<()> {
        let balances = self.client.get_balance(Some(&["USDT".to_string()])).await?;

        let usdt_balance = balances
            .iter()
            .find(|b| b.asset.eq_ignore_ascii_case("USDT"))
            .ok_or_else(|| {
                InfraError::Msg("Rest update account bal err: USDT balance missing".into())
            })?;

        self.total_equity = usdt_balance.total;
        info!("[WS] Rest update acc_order={:?}", usdt_balance);
        Ok(())
    }

    pub async fn rest_update_acc_pos_weight(
        &mut self,
        inst_infos: &HashMap<InstKey, InstrumentInfo>,
    ) -> InfraResult<()> {
        let positions = self.client.get_positions(None).await?;
        let mut notional_map: HashMap<String, f64> = HashMap::new();

        for pos in positions {
            let pos_notional = match &self.client {
                CexClients::BinanceUm(_) => pos.size * pos.mark_price,
                CexClients::Okx(_) => {
                    let inst_key = (pos.inst.clone(), Market::Okx);
                    if let Some(inst_info) = inst_infos.get(&inst_key) {
                        let ct_val = inst_info.contract_value.unwrap_or(1.0);
                        pos.size * pos.mark_price * ct_val
                    } else {
                        0.0
                    }
                },
                _ => 0.0,
            };

            self.inst_mark_price
                .insert(pos.inst.clone(), pos.mark_price);

            *notional_map.entry(pos.inst.clone()).or_insert(0.0) += pos_notional;
        }

        notional_map.iter().for_each(|(inst, &notional)| {
            let weight = if self.total_equity > f64::EPSILON {
                notional / self.total_equity
            } else {
                0.0
            };

            self.acc_weights.insert(inst.clone(), weight);
        });

        self.acc_weights
            .retain(|inst, _| notional_map.contains_key(inst));
        println!("[WS] Update acc_weights={:?}, total equity: {}", self.acc_weights, self.total_equity);
        Ok(())
    }

    async fn process_weight(
        &mut self,
        target_weights: &DashMap<String, (f64, f64)>,
        inst_infos: &HashMap<InstKey, InstrumentInfo>,
    ) -> InfraResult<()> {
        let (diffs, computed_target_weights) = self.compare_weights(target_weights);

        if !diffs.is_empty() {
            info!("\n================ ACCOUNT UPDATE ================");
            info!("Account ID       : {:?}", self.account_id);
            info!("Account balance  : {:?}", self.total_equity);
            info!("Account Weights  : {:?}", self.acc_weights);
            info!("Target R Weights : {:?}", target_weights);
            info!("Target C Weights : {:?}", computed_target_weights);
            info!("Diffs            : {:?}", diffs);
            info!("================================================\n");
        }

        match &self.client {
            CexClients::BinanceUm(_) => {
                for (inst, diff) in diffs.iter() {
                    let mark_price = match self.inst_mark_price.get(inst) {
                        Some(&price) => price,
                        None => {
                            warn!("Mark price not found for {} — skipping", inst);
                            continue;
                        },
                    };

                    let inst_key = (inst.clone(), Market::BinanceUmFutures);
                    let Some(binance_info) = inst_infos.get(&inst_key) else {
                        warn!("Binance info not found for {} — skipping", inst);
                        continue;
                    };

                    let side = if *diff > 0.0 {
                        OrderSide::BUY
                    } else {
                        OrderSide::SELL
                    };
                    let inst_notional = (diff * self.total_equity).abs();
                    if inst_notional < 6.0 {
                        warn!(
                            "Inst notional less than 6.0 USDT on Binance Um, inst notional: {}",
                            inst_notional,
                        );

                        continue;
                    }

                    let size =
                        match calc_binance_order_size(mark_price, inst_notional, binance_info) {
                            Ok(s) => s,
                            Err(e) => {
                                warn!(
                                    "Failed to calculate Binance order size for {}: {} — skipping",
                                    inst, e,
                                );

                                continue;
                            },
                        };

                    let order_info = OrderParams {
                        inst: inst.clone(),
                        size: size.clone(),
                        side: side.clone(),
                        order_type: OrderType::Market,
                        ..OrderParams::default()
                    };

                    println!("Binance order info: {:#?}", order_info);

                    match self.client.place_order(order_info).await {
                        Ok(_) => {
                            info!("Binance order placed successfully for {}", inst);

                            self.acc_weights
                                .entry(inst.clone())
                                .and_modify(|weight| *weight += *diff)
                                .or_insert(*diff);
                        },
                        Err(e) => {
                            warn!("Failed to place order for {}: {} — skipping", inst, e);
                        },
                    };
                }
            },
            CexClients::Okx(_) => {
                for (inst, diff) in diffs.iter() {
                    let mark_price = match self.inst_mark_price.get(inst) {
                        Some(&price) => price,
                        None => {
                            warn!("Mark price not found for {} — skipping", inst);
                            continue;
                        },
                    };

                    let inst_key = (inst.clone(), Market::Okx);
                    let Some(okx_info) = inst_infos.get(&inst_key) else {
                        warn!("Okx info not found for {} — skipping", inst);
                        continue;
                    };

                    let side = if *diff > 0.0 {
                        OrderSide::BUY
                    } else {
                        OrderSide::SELL
                    };
                    let inst_notional = (diff * self.total_equity).abs();

                    let size = match calc_okx_order_size(mark_price, inst_notional, okx_info) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(
                                "Failed to calculate OKX order size for {}: {} — skipping",
                                inst, e,
                            );

                            continue;
                        },
                    };

                    let order_info = OrderParams {
                        inst: inst.clone(),
                        size: size.clone(),
                        side: side.clone(),
                        order_type: OrderType::Market,
                        margin_mode: Some(MarginMode::Isolated),
                        ..Default::default()
                    };

                    println!("okx order info: {:#?}", order_info);

                    match self.client.place_order(order_info).await {
                        Ok(_) => {
                            info!("Okx order placed successfully for {}", inst);

                            self.acc_weights
                                .entry(inst.clone())
                                .and_modify(|weight| *weight += *diff)
                                .or_insert(*diff);
                        },
                        Err(e) => {
                            warn!("Failed to place order for {}: {} — skipping", inst, e);
                        },
                    };
                }
            },
            _ => {},
        };

        Ok(())
    }

    fn compare_weights(
        &mut self,
        target_weights: &DashMap<String, (f64, f64)>,
    ) -> (HashMap<String, f64>, HashMap<String, f64>) {
        let mut diffs = HashMap::new();
        let mut computed_target_weights = HashMap::new();

        let inst_count = target_weights.len().max(1) as f64;

        for r in target_weights.iter() {
            let inst = r.key();
            let (price, raw_weight) = *r.value();

            self.inst_mark_price.insert(inst.clone(), price);

            let target_w = raw_weight / inst_count;
            computed_target_weights.insert(inst.clone(), target_w);

            let current_w = self.acc_weights.get(inst).cloned().unwrap_or(0.0);
            let diff = target_w - current_w;

            if diff.abs() > 0.01 {
                diffs.insert(inst.clone(), diff);
            }
        }

        (diffs, computed_target_weights)
    }

    fn from_config(cfg: &AccountFileConfig, shared_client: Arc<Client>) -> InfraResult<Self> {
        let client = match cfg.exchange.to_lowercase().as_str() {
            "okx" => {
                let mut cli = OkxCli::new(shared_client);
                cli.api_key = Some(OkxKey {
                    api_key: cfg.api_key.clone(),
                    secret_key: cfg.api_secret.clone(),
                    passphrase: cfg.passphrase.clone().unwrap_or_default(),
                });
                CexClients::Okx(cli)
            },
            "binance_um" => {
                let mut cli = BinanceUmCli::new(shared_client);
                cli.api_key = Some(BinanceKey {
                    api_key: cfg.api_key.clone(),
                    secret_key: cfg.api_secret.clone(),
                });
                CexClients::BinanceUm(cli)
            },
            "binance_cm" => {
                let mut cli = BinanceCmCli::new(shared_client);
                cli.api_key = Some(BinanceKey {
                    api_key: cfg.api_key.clone(),
                    secret_key: cfg.api_secret.clone(),
                });
                CexClients::BinanceCm(cli)
            },
            e => return Err(InfraError::Msg(format!("Unknown exchange: {}", e))),
        };

        Ok(Self {
            account_id: cfg.account_id.clone(),
            client,
            acc_weights: HashMap::new(),
            inst_mark_price: HashMap::new(),
            total_equity: 0.0,
            account_orders_task_id: cfg.account_orders_task_id,
            account_bal_pos_task_id: cfg.account_bal_pos_task_id,
        })
    }

    fn config_changed(&self, other: &Self) -> bool {
        self.account_id != other.account_id
            || self.account_orders_task_id != other.account_orders_task_id
            || self.account_bal_pos_task_id != other.account_bal_pos_task_id
    }
}
