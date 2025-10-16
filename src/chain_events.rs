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
                let new_price: u64 = event.newPrice.to();
                let block_number: u64 = event.blockNumber.to();

                tracing::info!("📈 Price update: {} (block {})", new_price, block_number);

                let mut state_guard = state.write().await;
                state_guard.current_price = new_price;

                let msg = ServerMessage::PriceUpdate {
                    new_price,
                    block_number,
                };
                let _ = broadcast_tx.send(msg);
            }
            Some(&StockMarket::Position::SIGNATURE_HASH) => {
                let event = StockMarket::Position::decode_log(&log.inner, true)?;
                let user_addr = event.user;
                let balance: u64 = event.balance.to();
                let holdings: u64 = event.holdings.to();
                let block_number: u64 = event.blockNumber.to();

                tracing::info!("💼 Position update: {:?} | balance: {}, holdings: {} (block {})",
                    user_addr, balance, holdings, block_number);

                let mut state_guard = state.write().await;
                state_guard.balances.insert(user_addr, balance);
                state_guard.holdings.insert(user_addr, holdings);
                state_guard.last_position_block = block_number;

                let msg = ServerMessage::Position {
                    address: format!("{:?}", user_addr),
                    balance,
                    holdings,
                    block_number,
                };
                let _ = broadcast_tx.send(msg);
            }
            Some(&StockMarket::NewUser::SIGNATURE_HASH) => {
                let event = StockMarket::NewUser::decode_log(&log.inner, true)?;
                let user_addr = event.user;

                tracing::info!("👤 New user registered: {:?}", user_addr);

                let msg = ServerMessage::Position {
                    address: format!("{:?}", user_addr),
                    balance: 1000,
                    holdings: 0,
                    block_number: 0,
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
