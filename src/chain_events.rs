use crate::{AppState, ServerMessage, StockMarket};
use alloy::{rpc::types::Log, sol_types::SolEvent};
use anyhow::Result;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub async fn process_chain_events<S>(
    mut stream: S,
    state: Arc<RwLock<AppState>>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
) -> Result<()>
where
    S: StreamExt<Item = Log> + Unpin,
{
    while let Some(log) = stream.next().await {
        let tx_hash = log.transaction_hash.unwrap_or_default();
        let log_index = log.log_index.unwrap_or(0);

        {
            let mut state_guard = state.write().await;
            if state_guard.seen_logs.contains(&(tx_hash, log_index)) {
                continue;
            }
            state_guard.seen_logs.insert((tx_hash, log_index));
        }

        let topic0_opt = log.topic0().copied();
        let inner_log = log.inner;

        if let Some(topic0) = topic0_opt {
            if topic0 == StockMarket::PriceUpdate::SIGNATURE_HASH {
                if let Ok(decoded) = StockMarket::PriceUpdate::decode_log(&inner_log, true) {
                    let new_price = decoded.newPrice.to::<u64>();

                    tracing::info!(
                        "📊 PriceUpdate: {} → {} (block: {})",
                        decoded.oldPrice,
                        new_price,
                        decoded.blockNumber
                    );

                    {
                        let mut state_guard = state.write().await;
                        state_guard.current_price = new_price;
                    }

                    let msg = ServerMessage::PriceUpdate {
                        old_price: decoded.oldPrice.to::<u64>(),
                        new_price,
                        block_number: decoded.blockNumber.to::<u64>(),
                    };
                    let _ = broadcast_tx.send(msg);
                }
            } else if topic0 == StockMarket::Bought::SIGNATURE_HASH {
                if let Ok(decoded) = StockMarket::Bought::decode_log(&inner_log, true) {
                    let user_addr = decoded.user;
                    let balance = decoded.newBalance.to::<u64>();
                    let holdings = decoded.newHoldings.to::<u64>();

                    let (name, current_price) = {
                        let mut state_guard = state.write().await;
                        state_guard.balances.insert(user_addr, balance);
                        state_guard.holdings.insert(user_addr, holdings);
                        (
                            state_guard.names.get(&user_addr).cloned(),
                            state_guard.current_price,
                        )
                    };

                    tracing::info!(
                        "💰 Bought: user={:?}, amount={}, price={}, block={}, balance={}, holdings={}",
                        user_addr,
                        decoded.amount,
                        decoded.price,
                        decoded.blockNumber,
                        balance,
                        holdings,
                    );

                    let msg = ServerMessage::Bought {
                        user: format!("{:?}", user_addr),
                        name: name.clone(),
                        amount: decoded.amount.to::<u64>(),
                        price: decoded.price.to::<u64>(),
                        block_number: decoded.blockNumber.to::<u64>(),
                        balance,
                        holdings,
                    };
                    let _ = broadcast_tx.send(msg);

                    let position_msg = ServerMessage::Position {
                        address: format!("{:?}", user_addr),
                        name,
                        balance,
                        holdings,
                        current_price,
                    };
                    let _ = broadcast_tx.send(position_msg);
                }
            } else if topic0 == StockMarket::Sold::SIGNATURE_HASH {
                if let Ok(decoded) = StockMarket::Sold::decode_log(&inner_log, true) {
                    let user_addr = decoded.user;
                    let balance = decoded.newBalance.to::<u64>();
                    let holdings = decoded.newHoldings.to::<u64>();

                    let (name, current_price) = {
                        let mut state_guard = state.write().await;
                        state_guard.balances.insert(user_addr, balance);
                        state_guard.holdings.insert(user_addr, holdings);
                        (
                            state_guard.names.get(&user_addr).cloned(),
                            state_guard.current_price,
                        )
                    };

                    tracing::info!(
                        "💸 Sold: user={:?}, amount={}, price={}, block={}, balance={}, holdings={}",
                        user_addr,
                        decoded.amount,
                        decoded.price,
                        decoded.blockNumber,
                        balance,
                        holdings
                    );

                    let msg = ServerMessage::Sold {
                        user: format!("{:?}", user_addr),
                        name: name.clone(),
                        amount: decoded.amount.to::<u64>(),
                        price: decoded.price.to::<u64>(),
                        block_number: decoded.blockNumber.to::<u64>(),
                        balance,
                        holdings,
                    };
                    let _ = broadcast_tx.send(msg);

                    let position_msg = ServerMessage::Position {
                        address: format!("{:?}", user_addr),
                        name,
                        balance,
                        holdings,
                        current_price,
                    };
                    let _ = broadcast_tx.send(position_msg);
                }
            } else if topic0 == StockMarket::NewUser::SIGNATURE_HASH {
                if let Ok(decoded) = StockMarket::NewUser::decode_log(&inner_log, true) {
                    tracing::info!("👤 NewUser: {:?}", decoded.user);
                }
            }
        }
    }

    Ok(())
}
