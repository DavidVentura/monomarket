use crate::BackendTxEvent;
use crate::ws::ServerMessage;
use alloy::{
    network::TransactionBuilder,
    primitives::{Address, U256},
    providers::{Provider, WalletProvider},
    rpc::types::TransactionRequest,
    transports::Transport,
};
use anyhow::Result;
use tokio::sync::{broadcast, mpsc};

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

async fn handle_fund_event<T, P>(
    provider: &P,
    addr: Address,
    broadcast_tx: &broadcast::Sender<ServerMessage>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T> + WalletProvider,
{
    let balance = provider.get_balance(addr).await?;
    tracing::info!("Balance for {:?}: {} wei", addr, balance);

    if balance > U256::ZERO {
        tracing::info!("Address already funded, sending Funded event");
        let msg = ServerMessage::Funded {
            address: format!("{:?}", addr),
            amount: balance.to::<u64>(),
        };
        let _ = broadcast_tx.send(msg);
        return Ok(());
    }

    tracing::info!("Balance is zero, funding account...");
    let funding_amount = U256::from(500_000_000_000_000_000u64);
    tracing::info!("Funding {:?} with {} wei (0.5 MON)", addr, funding_amount);

    let gas_price = U256::from(0x21d664903cu64);
    let gas_limit = 25_000u64; // experimentally obtained 25k gas
    tracing::info!(
        "Gas cost for funding tx: {} wei",
        gas_price * U256::from(gas_limit)
    );

    let tx = TransactionRequest::default()
        .to(addr)
        .value(funding_amount)
        .with_gas_limit(gas_limit)
        .with_gas_price(gas_price.to::<u128>());

    let pending = provider.send_transaction(tx).await?;
    let tx_hash = *pending.tx_hash();
    tracing::info!("📤 Funding tx sent: {:?}", tx_hash);

    let receipt = pending.get_receipt().await?;
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
        let msg = ServerMessage::Funded {
            address: format!("{:?}", addr),
            amount: funding_amount.to::<u64>(),
        };
        let _ = broadcast_tx.send(msg);
    } else {
        let error_msg = format!("Funding transaction failed: {:?}", tx_hash);
        tracing::error!("{}", error_msg);
        let msg = ServerMessage::FundError {
            address: format!("{:?}", addr),
            error: error_msg,
        };
        let _ = broadcast_tx.send(msg);
    }

    Ok(())
}

async fn handle_tick_event<T, P>(
    contract: &StockMarket::StockMarketInstance<T, P>,
    broadcast_tx: &broadcast::Sender<ServerMessage>,
) -> Result<()>
where
    T: Transport + Clone,
    P: Provider<T>,
{
    let pending = contract.tick().send().await?;
    let tx_hash = *pending.tx_hash();
    tracing::info!("📤 Tick tx sent: {:?}", tx_hash);

    let receipt = pending.get_receipt().await?;
    tracing::info!(
        "✅ Tick tx confirmed: {:?} (block: {}, status: {})",
        tx_hash,
        receipt.block_number.unwrap_or_default(),
        if receipt.status() {
            "success"
        } else {
            "failed"
        }
    );

    if !receipt.status() {
        let error_msg = format!("Tick transaction failed: {:?}", tx_hash);
        tracing::error!("{}", error_msg);
        let msg = ServerMessage::TxError { error: error_msg };
        let _ = broadcast_tx.send(msg);
    }

    Ok(())
}

pub async fn backend_tx_executor<T, P>(
    mut rx: mpsc::Receiver<BackendTxEvent>,
    provider: P,
    contract_addr: Address,
    broadcast_tx: broadcast::Sender<ServerMessage>,
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
            BackendTxEvent::Fund(addr) => {
                tracing::info!("Processing Fund event for {:?}", addr);
                if let Err(e) = handle_fund_event(&provider, addr, &broadcast_tx).await {
                    let error_msg = format!("Failed to fund account: {}", e);
                    tracing::error!("{}", error_msg);
                    let msg = ServerMessage::FundError {
                        address: format!("{:?}", addr),
                        error: error_msg,
                    };
                    let _ = broadcast_tx.send(msg);
                }
            }
            BackendTxEvent::Tick => {
                tracing::info!("Processing Tick event");
                if let Err(e) = handle_tick_event(&contract, &broadcast_tx).await {
                    let error_msg = format!("Failed to process tick: {}", e);
                    tracing::error!("{}", error_msg);
                    let msg = ServerMessage::TxError { error: error_msg };
                    let _ = broadcast_tx.send(msg);
                }
            }
        }
    }

    Ok(())
}

