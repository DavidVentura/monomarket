use crate::ws::ServerMessage;
use crate::{AppState, BackendTxEvent};
use alloy::{
    network::TransactionBuilder,
    primitives::{Address, U256},
    providers::{Provider, WalletProvider},
    rpc::types::TransactionRequest,
    transports::Transport,
};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};

mod contract {
    use alloy::sol;
    sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        StockMarket,
        "contract/out/StockMarket.sol/StockMarket.json"
    );
}

pub use contract::StockMarket;

async fn handle_fund_event<'a, T, P>(
    provider: &P,
    contract: &StockMarket::StockMarketInstance<T, &'a P>,
    addr: Address,
    broadcast_tx: &broadcast::Sender<ServerMessage>,
    client_tx: &mpsc::Sender<ServerMessage>,
    state: Arc<RwLock<AppState>>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T> + WalletProvider,
{
    let balance = provider.get_balance(addr).await?;
    tracing::info!("Balance for {:?}: {} wei", addr, balance);

    if balance > U256::ZERO {
        tracing::info!(
            "Address already funded, reading holdings and sending Funded and Position events"
        );
        let holdings = contract.getHoldings(addr).call().await?;
        let contract_balance = contract.getBalance(addr).call().await?;

        let funded_msg = ServerMessage::Funded {
            address: format!("{:?}", addr),
            amount: balance.to::<u64>(),
        };
        let _ = client_tx.send(funded_msg).await;

        let balance = contract_balance._0.to::<u64>();
        let holdings = holdings._0.to::<u64>();
        if balance > 0 && holdings > 0 {
            let position_msg = ServerMessage::Position {
                address: format!("{:?}", addr),
                balance,
                holdings,
                block_number: 0,
            };
            let _ = broadcast_tx.send(position_msg);
        }
        return Ok(());
    }

    tracing::info!("Balance is zero, funding account...");
    let funding_amount = U256::from(500_000_000_000_000_000u64);
    tracing::info!("Funding {:?} with {} wei (0.5 MON)", addr, funding_amount);

    let nonce = {
        let mut state_guard = state.write().await;
        let nonce = state_guard.backend_nonce;
        state_guard.backend_nonce += 1;
        nonce
    };

    let gas_price = U256::from(0x21d664903cu64);
    let gas_limit = 25_000u64; // experimentally obtained 25k gas
    tracing::info!(
        "Gas cost for funding tx: {} wei",
        gas_price * U256::from(gas_limit)
    );

    let tx = TransactionRequest::default()
        .to(addr)
        .value(funding_amount)
        .with_nonce(nonce)
        .with_gas_limit(gas_limit)
        .with_gas_price(gas_price.to::<u128>());

    let pending = provider.send_transaction(tx).await?;
    let tx_hash = *pending.tx_hash();
    tracing::info!("📤 Funding tx sent: {:?} (nonce: {})", tx_hash, nonce);

    let receipt = loop {
        match provider.get_transaction_receipt(tx_hash).await? {
            Some(receipt) => break receipt,
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    };
    tracing::info!(
        "✅ Funding tx confirmed: {:?} (block: {}, status: {})",
        tx_hash,
        receipt.block_number.unwrap_or_default(),
        if receipt.status() {
            "success"
        } else {
            "failed"
        }
    );

    if receipt.status() {
        let funded_msg = ServerMessage::Funded {
            address: format!("{:?}", addr),
            amount: funding_amount.to::<u64>(),
        };
        let _ = client_tx.send(funded_msg).await;
    } else {
        let error_msg = format!("Funding transaction failed: {:?}", tx_hash);
        tracing::error!("{}", error_msg);
        let msg = ServerMessage::FundError {
            address: format!("{:?}", addr),
            error: error_msg,
        };
        let _ = client_tx.send(msg).await;
    }

    Ok(())
}

async fn handle_tick_event<T, P>(
    provider: &P,
    contract: &StockMarket::StockMarketInstance<T, &P>,
    state: Arc<RwLock<AppState>>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T> + WalletProvider,
{
    let nonce = {
        let mut state_guard = state.write().await;
        let nonce = state_guard.backend_nonce;
        state_guard.backend_nonce += 1;
        nonce
    };

    let max_fee_per_gas = U256::from(0x21d664903cu64);
    let max_priority_fee = U256::from(1_000_000_000u64);
    let gas_limit = 60_000u64;

    let call = contract.tick();
    let tx_req = call
        .into_transaction_request()
        .with_nonce(nonce)
        .with_gas_limit(gas_limit)
        .with_max_fee_per_gas(max_fee_per_gas.to::<u128>())
        .with_max_priority_fee_per_gas(max_priority_fee.to::<u128>());

    let pending = provider.send_transaction(tx_req).await?;
    let tx_hash = *pending.tx_hash();
    tracing::info!("📤 Tick tx sent: {:?} (nonce: {})", tx_hash, nonce);

    Ok(())
}

pub async fn handle_restart_game<T, P>(
    provider: P,
    contract_addr: Address,
    state: Arc<RwLock<AppState>>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T> + WalletProvider,
{
    tracing::info!("🔄 Starting game restart sequence");

    let contract = StockMarket::new(contract_addr, &provider);

    // Step 1: Fund all players back to 0.5 MON
    tracing::info!("Step 1: Funding all players to 0.5 MON");
    let funding_amount = U256::from(500_000_000_000_000_000u64); // 0.5 MON
    let min_balance = U256::from(450_000_000_000_000_000u64); // 0.45 MON threshold

    let addresses: Vec<Address> = {
        let state_guard = state.read().await;
        state_guard.names.keys().copied().collect()
    };

    tracing::info!("Found {} players to fund", addresses.len());

    for addr in addresses {
        let balance = provider.get_balance(addr).await?;

        if balance < min_balance {
            tracing::info!("Funding {:?} (current: {} wei)", addr, balance);

            let nonce = {
                let mut state_guard = state.write().await;
                let nonce = state_guard.backend_nonce;
                state_guard.backend_nonce += 1;
                nonce
            };

            let gas_price = U256::from(0x21d664903cu64);
            let gas_limit = 25_000u64;

            let tx = alloy::rpc::types::TransactionRequest::default()
                .to(addr)
                .value(funding_amount - balance)
                .with_nonce(nonce)
                .with_gas_limit(gas_limit)
                .with_gas_price(gas_price.to::<u128>());

            let pending = provider.send_transaction(tx).await?;
            let tx_hash = *pending.tx_hash();
            tracing::info!("📤 Funding tx sent: {:?} (nonce: {})", tx_hash, nonce);

            // Wait for confirmation
            loop {
                match provider.get_transaction_receipt(tx_hash).await? {
                    Some(receipt) => {
                        tracing::info!(
                            "✅ Funding confirmed: {:?} (status: {})",
                            tx_hash,
                            receipt.status()
                        );
                        if receipt.status() {
                            let funded_msg = ServerMessage::Funded {
                                address: format!("{:?}", addr),
                                amount: funding_amount.to::<u64>(),
                            };
                            let _ = broadcast_tx.send(funded_msg);
                        }
                        break;
                    }
                    None => {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
            }
        } else {
            tracing::info!("Skipping {:?} (sufficient balance: {} wei)", addr, balance);
        }
    }

    // Step 2: Call reset() on contract
    tracing::info!("Step 2: Calling reset() on contract");
    let nonce = {
        let mut state_guard = state.write().await;
        let nonce = state_guard.backend_nonce;
        state_guard.backend_nonce += 1;
        nonce
    };

    let max_fee_per_gas = U256::from(0x21d664903cu64);
    let max_priority_fee = U256::from(1_000_000_000u64);
    let gas_limit = 500_000u64;

    let call = contract.reset();
    let tx_req = call
        .into_transaction_request()
        .with_nonce(nonce)
        .with_gas_limit(gas_limit)
        .with_max_fee_per_gas(max_fee_per_gas.to::<u128>())
        .with_max_priority_fee_per_gas(max_priority_fee.to::<u128>());

    let pending = provider.send_transaction(tx_req).await?;
    let reset_tx_hash = *pending.tx_hash();
    tracing::info!("📤 Reset tx sent: {:?} (nonce: {})", reset_tx_hash, nonce);

    // Step 3: Wait for reset to complete
    tracing::info!("Step 3: Waiting for reset confirmation");
    loop {
        match provider.get_transaction_receipt(reset_tx_hash).await? {
            Some(receipt) => {
                if receipt.status() {
                    tracing::info!("✅ Reset confirmed: {:?}", reset_tx_hash);
                } else {
                    tracing::error!("❌ Reset failed: {:?}", reset_tx_hash);
                    return Err(anyhow::anyhow!("Reset transaction failed"));
                }
                break;
            }
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }

    let game_duration = 50; // TODO
    // Step 4: Call start() on contract
    tracing::info!("Step 4: Starting new game ({game_duration} blocks)");
    let nonce = {
        let mut state_guard = state.write().await;
        let nonce = state_guard.backend_nonce;
        state_guard.backend_nonce += 1;
        nonce
    };

    let call = contract.start(U256::from(game_duration));
    let tx_req = call
        .into_transaction_request()
        .with_nonce(nonce)
        .with_gas_limit(gas_limit)
        .with_max_fee_per_gas(max_fee_per_gas.to::<u128>())
        .with_max_priority_fee_per_gas(max_priority_fee.to::<u128>());

    let pending = provider.send_transaction(tx_req).await?;
    let start_tx_hash = *pending.tx_hash();
    tracing::info!("📤 Start tx sent: {:?} (nonce: {})", start_tx_hash, nonce);

    loop {
        match provider.get_transaction_receipt(start_tx_hash).await? {
            Some(receipt) => {
                if receipt.status() {
                    tracing::info!("✅ Game started: {:?}", start_tx_hash);
                } else {
                    tracing::error!("❌ Start failed: {:?}", start_tx_hash);
                    return Err(anyhow::anyhow!("Start transaction failed"));
                }
                break;
            }
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }

    tracing::info!("🎮 Game restart sequence completed successfully");
    Ok(())
}

pub async fn backend_tx_executor<T, P>(
    mut rx: mpsc::Receiver<BackendTxEvent>,
    provider: P,
    contract_addr: Address,
    broadcast_tx: broadcast::Sender<ServerMessage>,
    state: Arc<RwLock<AppState>>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T> + WalletProvider,
{
    let contract = StockMarket::new(contract_addr, &provider);

    let backend_addr = provider.default_signer_address();
    tracing::info!("Backend wallet address: {:?}", backend_addr);
    while let Some(event) = rx.recv().await {
        match event {
            BackendTxEvent::Fund(addr, client_tx) => {
                tracing::info!("Processing Fund event for {:?}", addr);
                if let Err(e) = handle_fund_event(
                    &provider,
                    &contract,
                    addr,
                    &broadcast_tx,
                    &client_tx,
                    state.clone(),
                )
                .await
                {
                    let error_msg = format!("Failed to fund account: {}", e);
                    tracing::error!("{}", error_msg);
                    let msg = ServerMessage::FundError {
                        address: format!("{:?}", addr),
                        error: error_msg,
                    };
                    let _ = client_tx.send(msg).await;
                }
            }
            BackendTxEvent::GameOver => {
                let _ = broadcast_tx.send(ServerMessage::GameEnded);
            }
            BackendTxEvent::Tick => {
                tracing::info!("Processing Tick event");
                if let Err(e) = handle_tick_event(&provider, &contract, state.clone()).await {
                    let error_msg = format!("Failed to process tick: {}", e);
                    tracing::error!("{}", error_msg);

                    if error_msg.contains("Already ticked this block") {
                        tracing::debug!("Block was already ticked (race condition, expected)");
                    } else if error_msg.contains("higher priority") {
                        tracing::warn!("⚠️  Higher priority transaction exists, jumping nonce +20");
                        let mut state_guard = state.write().await;
                        let old_nonce = state_guard.backend_nonce;
                        state_guard.backend_nonce += 20;
                        tracing::info!(
                            "✅ Nonce jumped: {} -> {} (skipping stuck transactions)",
                            old_nonce,
                            state_guard.backend_nonce
                        );
                    } else {
                        tracing::warn!(
                            "⚠️  Tick transaction failed, resyncing nonce from chain..."
                        );
                        let backend_address = provider.default_signer_address();
                        match provider.get_transaction_count(backend_address).await {
                            Ok(chain_nonce) => {
                                let mut state_guard = state.write().await;
                                let old_nonce = state_guard.backend_nonce;

                                if chain_nonce > old_nonce {
                                    state_guard.backend_nonce = chain_nonce;
                                    tracing::info!(
                                        "✅ Nonce resynced upward: {} -> {} (next block will retry)",
                                        old_nonce,
                                        chain_nonce
                                    );
                                } else if chain_nonce == old_nonce {
                                    tracing::info!("✅ Nonce already in sync: {}", chain_nonce);
                                } else {
                                    tracing::warn!(
                                        "⚠️  Chain nonce ({}) < local nonce ({}), keeping local (transactions pending)",
                                        chain_nonce,
                                        old_nonce
                                    );
                                }
                            }
                            Err(nonce_err) => {
                                tracing::error!("Failed to fetch nonce from chain: {}", nonce_err);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
