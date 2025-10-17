use crate::{ws::*, AppState, BackendTxEvent, GasCosts};
use alloy::{
    primitives::{Address, Bytes},
    providers::{Provider, WalletProvider},
    transports::Transport,
};
use anyhow::Result;
use axum::extract::ws::{Message as AxumMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

pub async fn handle_axum_connection<T, P>(
    socket: WebSocket,
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
    P: Provider<T> + WalletProvider + Clone + 'static,
{
    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(100);
    let (mut ws_sender, mut ws_receiver) = socket.split();

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
        ws_sender.send(AxumMessage::Text(json)).await?;
        tracing::info!("Sent connection info to client");

        let state_guard = state.read().await;

        let current_price_msg = ServerMessage::CurrentPrice {
            price: state_guard.current_price,
        };
        let json = serde_json::to_string(&current_price_msg)?;
        ws_sender.send(AxumMessage::Text(json)).await?;
        tracing::info!("Sent current price {} to client", state_guard.current_price);

        let current_block_msg = ServerMessage::CurrentBlockHeight {
            height: state_guard.current_block_height,
        };
        let json = serde_json::to_string(&current_block_msg)?;
        ws_sender.send(AxumMessage::Text(json)).await?;
        tracing::info!(
            "Sent current block height {} to client",
            state_guard.current_block_height
        );

        if let (Some(start_block), Some(end_block)) =
            (state_guard.game_start_block, state_guard.game_end_block)
        {
            let current_height = state_guard.current_block_height;

            if current_height <= end_block {
                let game_started_msg = ServerMessage::GameStarted {
                    start_height: start_block,
                    end_height: end_block,
                };
                let json = serde_json::to_string(&game_started_msg)?;
                ws_sender.send(AxumMessage::Text(json)).await?;
                tracing::info!(
                    "Sent game lifecycle info to client: {} to {} (current: {})",
                    start_block,
                    end_block,
                    current_height
                );
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
            ws_sender.send(AxumMessage::Text(json)).await?;
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
            ws_sender.send(AxumMessage::Text(json)).await?;
        }
    }

    let state_clone = state.clone();
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(msg) = broadcast_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if ws_sender.send(AxumMessage::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
                Some(msg) = client_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if ws_sender.send(AxumMessage::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(AxumMessage::Text(text)) => {
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
                        ClientMessage::RestartGame => {
                            tracing::info!("🔄 Restart game request received");
                            let provider_clone = provider.clone();
                            let state_clone = state_clone.clone();
                            let broadcast_tx_clone = broadcast_tx.clone();
                            tokio::spawn(async move {
                                if let Err(e) = crate::backend::handle_restart_game(
                                    provider_clone,
                                    contract_address,
                                    state_clone,
                                    broadcast_tx_clone,
                                )
                                .await
                                {
                                    tracing::error!("Failed to restart game: {}", e);
                                }
                            });
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to parse client message: {}", e);
                    }
                }
            }
            Ok(AxumMessage::Close(_)) => break,
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
