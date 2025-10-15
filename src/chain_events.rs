use crate::{AppState, backend::StockMarket, ws::ServerMessage};
use alloy::{rpc::types::Log, sol_types::SolEvent};
use futures_util::{Stream, StreamExt};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

pub async fn process_chain_events(
    mut stream: impl Stream<Item = Log> + Unpin,
    state: Arc<RwLock<AppState>>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
) -> anyhow::Result<()> {
    while let Some(log) = stream.next().await {
        let key = (log.transaction_hash.unwrap(), log.log_index.unwrap());

        {
            let mut state_guard = state.write().await;
            if state_guard.seen_logs.contains(&key) {
                continue;
            }
            state_guard.seen_logs.insert(key);
        }

        match log.topic0() {
            Some(&StockMarket::PriceUpdate::SIGNATURE_HASH) => {
                let event = StockMarket::PriceUpdate::decode_log(&log.inner, true)?;
                let mut state_guard = state.write().await;
                state_guard.current_price = event.newPrice.to();

                let msg = ServerMessage::PriceUpdate {
                    old_price: event.oldPrice.to(),
                    new_price: event.newPrice.to(),
                    block_number: event.blockNumber.to(),
                };
                let _ = broadcast_tx.send(msg);
            }
            Some(&StockMarket::Bought::SIGNATURE_HASH) => {
                let event = StockMarket::Bought::decode_log(&log.inner, true)?;
                let user_addr = event.user;
                let amount: u64 = event.amount.to();

                let mut state_guard = state.write().await;
                state_guard
                    .balances
                    .insert(user_addr, event.newBalance.to());
                state_guard
                    .holdings
                    .insert(user_addr, event.newHoldings.to());
                let name = state_guard.names.get(&user_addr).cloned();

                let msg = ServerMessage::Bought {
                    user: format!("{:?}", user_addr),
                    name,
                    amount,
                    balance: event.newBalance.to(),
                    holdings: event.newHoldings.to(),
                };
                let _ = broadcast_tx.send(msg);
            }
            Some(&StockMarket::Sold::SIGNATURE_HASH) => {
                let event = StockMarket::Sold::decode_log(&log.inner, true)?;
                let user_addr = event.user;
                let amount: u64 = event.amount.to();

                let mut state_guard = state.write().await;
                state_guard
                    .balances
                    .insert(user_addr, event.newBalance.to());
                state_guard
                    .holdings
                    .insert(user_addr, event.newHoldings.to());
                let name = state_guard.names.get(&user_addr).cloned();

                let msg = ServerMessage::Sold {
                    user: format!("{:?}", user_addr),
                    name,
                    amount,
                    balance: event.newBalance.to(),
                    holdings: event.newHoldings.to(),
                };
                let _ = broadcast_tx.send(msg);
            }
            Some(other) => {
                tracing::error!("Unexpected event {other:?}");
            }
            None => {
                tracing::error!("Unexpected None");
            }
        }
    }
    Ok(())
}
