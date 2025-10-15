use crate::{AppState, BackendTxEvent, GasCosts};
use alloy::{primitives::{Address, Bytes}, providers::{Provider, WalletProvider}, transports::Transport};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{net::TcpStream, sync::{broadcast, mpsc, RwLock}};
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
        old_price: u64,
        new_price: u64,
        block_number: u64,
    },
    Bought {
        user: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        amount: u64,
        price: u64,
        block_number: u64,
        balance: u64,
        holdings: u64,
    },
    Sold {
        user: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        amount: u64,
        price: u64,
        block_number: u64,
        balance: u64,
        holdings: u64,
    },
    NameSet {
        address: String,
        name: String,
    },
    Position {
        address: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        balance: u64,
        holdings: u64,
        current_price: u64,
    },
    TxError {
        error: String,
    },
    NonceResponse {
        address: String,
        nonce: u64,
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
            let name = state_guard.names.get(address).cloned();
            let msg = ServerMessage::Position {
                address: format!("{:?}", address),
                name,
                balance: *balance,
                holdings,
                current_price: state_guard.current_price,
            };
            let json = serde_json::to_string(&msg)?;
            ws_sender.send(Message::Text(json)).await?;
        }
    }

    let state_clone = state.clone();
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = broadcast_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_sender.send(Message::Text(json)).await.is_err() {
                    break;
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

                                    let _ =
                                        backend_tx_sender.send(BackendTxEvent::Fund(addr)).await;

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
                                        tracing::info!(
                                            "Raw tx submitted: {:?}",
                                            pending_tx.tx_hash()
                                        );
                                    }
                                    Err(e) => {
                                        let error_msg =
                                            format!("Failed to submit transaction: {}", e);
                                        tracing::error!("{}", error_msg);

                                        let msg = ServerMessage::TxError { error: error_msg };
                                        let _ = broadcast_tx.send(msg);
                                    }
                                },
                                Err(e) => {
                                    let error_msg = format!("Failed to parse transaction: {}", e);
                                    tracing::error!("{}", error_msg);

                                    let msg = ServerMessage::TxError { error: error_msg };
                                    let _ = broadcast_tx.send(msg);
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
                                        let _ = broadcast_tx.send(msg);
                                    }
                                    Err(e) => {
                                        let error_msg = format!("Failed to get nonce: {}", e);
                                        tracing::error!("{}", error_msg);

                                        let msg = ServerMessage::TxError { error: error_msg };
                                        let _ = broadcast_tx.send(msg);
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
