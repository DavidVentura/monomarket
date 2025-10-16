use crate::{AppState, BackendTxEvent, GasCosts};
use alloy::{
    primitives::{Address, Bytes},
    providers::{Provider, WalletProvider},
    transports::Transport,
};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{
    net::TcpStream,
    sync::{RwLock, broadcast, mpsc},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    SetName { name: String, address: String },
    RawTx { raw_tx: String },
    GetNonce { address: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    ConnectionInfo {
        contract_address: String,
        gas_costs: GasInfo,
    },
    PriceUpdate {
        new_price: u64,
        block_number: u64,
    },
    CurrentPrice {
        price: u64,
    },
    NameSet {
        address: String,
        name: String,
    },
    Position {
        address: String,
        balance: u64,
        holdings: u64,
        block_number: u64,
    },
    TxError {
        error: String,
    },
    NonceResponse {
        address: String,
        nonce: u64,
    },
    Funded {
        address: String,
        amount: u64,
    },
    FundError {
        address: String,
        error: String,
    },
    TxSubmitted {
        tx_hash: String,
    },
    GameStarted {
        start_height: u64,
        end_height: u64,
    },
    GameEnded,
    CurrentBlockHeight {
        height: u64,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct GasInfo {
    pub register: u64,
    pub buy: u64,
    pub sell: u64,
}

pub async fn handle_connection<T, P>(
    stream: TcpStream,
    state: Arc<RwLock<AppState>>,
    mut broadcast_rx: broadcast::Receiver<ServerMessage>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
    provider: P,
    gas_costs: Arc<GasCosts>,
    contract_address: Address,
    backend_tx_sender: mpsc::Sender<BackendTxEvent>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T> + WalletProvider,
{
    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(100);
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!("Failed to accept WebSocket connection: {}", e);
            return Err(e.into());
        }
    };
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    {
        let connection_info = ServerMessage::ConnectionInfo {
            contract_address: format!("{:?}", contract_address),
            gas_costs: GasInfo {
                register: gas_costs.register,
                buy: gas_costs.buy,
                sell: gas_costs.sell,
            },
        };
        let json = serde_json::to_string(&connection_info)?;
        ws_sender.send(Message::Text(json)).await?;
        tracing::info!("Sent connection info to client");

        let state_guard = state.read().await;

        let current_price_msg = ServerMessage::CurrentPrice {
            price: state_guard.current_price,
        };
        let json = serde_json::to_string(&current_price_msg)?;
        ws_sender.send(Message::Text(json)).await?;
        tracing::info!("Sent current price {} to client", state_guard.current_price);

        let current_block_msg = ServerMessage::CurrentBlockHeight {
            height: state_guard.current_block_height,
        };
        let json = serde_json::to_string(&current_block_msg)?;
        ws_sender.send(Message::Text(json)).await?;
        tracing::info!("Sent current block height {} to client", state_guard.current_block_height);

        if let (Some(start_block), Some(end_block)) = (state_guard.game_start_block, state_guard.game_end_block) {
            let current_height = state_guard.current_block_height;

            if current_height > end_block {
                let game_ended_msg = ServerMessage::GameEnded;
                let json = serde_json::to_string(&game_ended_msg)?;
                ws_sender.send(Message::Text(json)).await?;
                tracing::info!("Sent game ended to client (current: {}, end: {})", current_height, end_block);
            } else {
                let game_started_msg = ServerMessage::GameStarted {
                    start_height: start_block,
                    end_height: end_block,
                };
                let json = serde_json::to_string(&game_started_msg)?;
                ws_sender.send(Message::Text(json)).await?;
                tracing::info!("Sent game lifecycle info to client: {} to {} (current: {})", start_block, end_block, current_height);
            }
        }
        let name_count = state_guard.names.len();
        if name_count > 0 {
            tracing::info!(
                "Sending {} existing name mappings to new client",
                name_count
            );
        }
        for (address, name) in state_guard.names.iter() {
            let msg = ServerMessage::NameSet {
                address: format!("{:?}", address),
                name: name.clone(),
            };
            let json = serde_json::to_string(&msg)?;
            ws_sender.send(Message::Text(json)).await?;
        }

        tracing::info!(
            "Sending {} position updates to new client",
            state_guard.balances.len()
        );
        for (address, balance) in state_guard.balances.iter() {
            let holdings = state_guard.holdings.get(address).copied().unwrap_or(0);
            let msg = ServerMessage::Position {
                address: format!("{:?}", address),
                balance: *balance,
                holdings,
                block_number: 0,
            };
            let json = serde_json::to_string(&msg)?;
            ws_sender.send(Message::Text(json)).await?;
        }
    }

    let state_clone = state.clone();
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(msg) = broadcast_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if ws_sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
                Some(msg) = client_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if ws_sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                tracing::debug!("Received WebSocket message: {}", text);
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(client_msg) => match client_msg {
                        ClientMessage::SetName { name, address } => {
                            match address.parse::<Address>() {
                                Ok(addr) => {
                                    tracing::info!("Setting name: {} → {}", address, name);

                                    {
                                        let mut state_guard = state_clone.write().await;
                                        state_guard.names.insert(addr, name.clone());
                                    }

                                    let msg = ServerMessage::NameSet {
                                        address: format!("{:?}", addr),
                                        name,
                                    };
                                    let _ = broadcast_tx.send(msg);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to parse address '{}': {}", address, e);
                                }
                            }
                        }
                        ClientMessage::RawTx { raw_tx } => {
                            tracing::info!(
                                "Received raw tx: {}...",
                                &raw_tx[..20.min(raw_tx.len())]
                            );

                            match raw_tx.parse::<Bytes>() {
                                Ok(bytes) => match provider.send_raw_transaction(&bytes).await {
                                    Ok(pending_tx) => {
                                        let tx_hash = *pending_tx.tx_hash();
                                        tracing::info!("📤 Raw tx submitted: {:?}", tx_hash);

                                        let msg = ServerMessage::TxSubmitted {
                                            tx_hash: format!("{:?}", tx_hash),
                                        };
                                        let _ = client_tx.send(msg).await;
                                    }
                                    Err(e) => {
                                        let error_msg =
                                            format!("Failed to submit transaction: {}", e);
                                        tracing::error!("{}", error_msg);

                                        let msg = ServerMessage::TxError { error: error_msg };
                                        let _ = client_tx.send(msg).await;
                                    }
                                },
                                Err(e) => {
                                    let error_msg = format!("Failed to parse transaction: {}", e);
                                    tracing::error!("{}", error_msg);

                                    let msg = ServerMessage::TxError { error: error_msg };
                                    let _ = client_tx.send(msg).await;
                                }
                            }
                        }
                        ClientMessage::GetNonce { address } => match address.parse::<Address>() {
                            Ok(addr) => {
                                tracing::info!("Getting nonce for address: {}", address);

                                match provider.get_transaction_count(addr).await {
                                    Ok(nonce) => {
                                        tracing::info!("Nonce for {}: {}", address, nonce);

                                        let msg = ServerMessage::NonceResponse {
                                            address: format!("{:?}", addr),
                                            nonce,
                                        };
                                        let _ = client_tx.send(msg).await;
                                        let _ = backend_tx_sender
                                            .send(BackendTxEvent::Fund(addr, client_tx.clone()))
                                            .await;
                                    }
                                    Err(e) => {
                                        let error_msg = format!("Failed to get nonce: {}", e);
                                        tracing::error!("{}", error_msg);

                                        let msg = ServerMessage::TxError { error: error_msg };
                                        let _ = client_tx.send(msg).await;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse address '{}': {}", address, e);
                            }
                        },
                    },
                    Err(e) => {
                        tracing::error!("Failed to parse client message: {}", e);
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    send_task.abort();
    Ok(())
}
