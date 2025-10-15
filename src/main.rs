mod backend;
mod chain_events;
mod ws;

use alloy::{
    network::EthereumWallet,
    primitives::{Address, TxHash},
    providers::{Provider, ProviderBuilder, WalletProvider, WsConnect},
    rpc::types::{Filter, Log},
    signers::local::PrivateKeySigner,
    transports::Transport,
};
use anyhow::Result;
use futures_util::StreamExt;
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tokio::{
    net::TcpListener,
    sync::{RwLock, broadcast, mpsc},
};
use ws::ServerMessage;

#[derive(Debug, Clone)]
pub enum BackendTxEvent {
    Fund(Address),
    Tick,
}

pub struct AppState {
    pub names: HashMap<Address, String>,
    pub seen_logs: HashSet<(TxHash, u64)>,
    pub current_price: u64,
    pub balances: HashMap<Address, u64>,
    pub holdings: HashMap<Address, u64>,
}

impl AppState {
    fn new() -> Self {
        Self {
            names: HashMap::new(),
            seen_logs: HashSet::new(),
            current_price: 50,
            balances: HashMap::new(),
            holdings: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GasCosts {
    pub register: u64,
    pub buy: u64,
    pub sell: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let rpc_url = env::var("RPC_URL").expect("RPC_URL not set");
    let contract_address = env::var("CONTRACT_ADDRESS").expect("CONTRACT_ADDRESS not set");
    let private_key = env::var("PRIVATE_KEY").expect("PRIVATE_KEY not set");
    let ws_port = env::var("WEBSOCKET_PORT").unwrap_or_else(|_| "8090".to_string());

    tracing::info!("Connecting to RPC: {}", rpc_url);
    tracing::info!("Contract address: {}", contract_address);
    tracing::info!("WebSocket server port: {}", ws_port);

    let signer = PrivateKeySigner::from_bytes(&private_key.parse()?)?;
    let wallet = EthereumWallet::from(signer);

    let ws_read = WsConnect::new(&rpc_url);
    let provider_read = ProviderBuilder::new().on_ws(ws_read).await?;

    let ws_write = WsConnect::new(&rpc_url);
    let provider_write = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_ws(ws_write)
        .await?;

    let contract_addr: Address = contract_address.parse()?;

    tracing::info!("Calculating gas costs...");

    let register_gas = 115_000;
    tracing::info!("Register costs...");
    let buy_gas = 35_529;
    let sell_gas = 35_529;

    let gas_costs = GasCosts {
        register: register_gas,
        buy: buy_gas,
        sell: sell_gas,
    };

    tracing::info!("Gas costs calculated:");
    tracing::info!("  register: {} gas", gas_costs.register);
    tracing::info!("  buy: {} gas", gas_costs.buy);
    tracing::info!("  sell: {} gas", gas_costs.sell);

    let gas_costs = Arc::new(gas_costs);

    let state = Arc::new(RwLock::new(AppState::new()));
    let (broadcast_tx, _) = broadcast::channel::<ServerMessage>(1000);
    let (backend_tx_sender, backend_tx_receiver) = mpsc::channel::<BackendTxEvent>(100);

    let provider_write_clone = provider_write.clone();
    let broadcast_tx_clone = broadcast_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = backend::backend_tx_executor(
            backend_tx_receiver,
            provider_write_clone,
            contract_addr,
            broadcast_tx_clone,
        )
        .await
        {
            tracing::error!("Backend tx executor error: {}", e);
        }
    });

    let state_clone = state.clone();
    let broadcast_tx_clone = broadcast_tx.clone();
    let provider_write_clone = provider_write.clone();
    let gas_costs_clone = gas_costs.clone();
    let backend_tx_sender_clone = backend_tx_sender.clone();
    tokio::spawn(async move {
        if let Err(e) = run_websocket_server(
            &ws_port,
            state_clone,
            broadcast_tx_clone,
            provider_write_clone,
            gas_costs_clone,
            contract_addr,
            backend_tx_sender_clone,
        )
        .await
        {
            tracing::error!("WebSocket server error: {}", e);
        }
    });

    tracing::info!("Starting block subscriber...");

    let block_sub = provider_read.subscribe_blocks().await?;
    let mut block_stream = block_sub.into_stream();

    tokio::spawn(async move {
        while let Some(block) = block_stream.next().await {
            tracing::info!(
                "🧱 New Block: {} (timestamp: {})",
                block.number,
                block.timestamp
            );
        }
    });

    tracing::info!("Starting event listener (using monadLogs for lower latency)...");

    let filter = Filter::new().address(contract_addr);

    let client = provider_read.client();
    let subscription_id: String = client
        .request("eth_subscribe", ("monadLogs", &filter))
        .await?;

    let sub = provider_read
        .get_subscription::<Log>(subscription_id.parse()?)
        .await?;
    let stream = sub.into_stream();

    tracing::info!("Subscribed to contract logs (monadLogs) and blocks!");

    chain_events::process_chain_events(stream, state, broadcast_tx).await
}

async fn run_websocket_server<T, P>(
    port: &str,
    state: Arc<RwLock<AppState>>,
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
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("WebSocket server listening on {}", addr);

    while let Ok((stream, addr)) = listener.accept().await {
        tracing::info!("Accepting connection from: {}", addr);
        let state_clone = state.clone();
        let broadcast_rx = broadcast_tx.subscribe();
        let broadcast_tx_clone = broadcast_tx.clone();
        let provider_clone = provider.clone();
        let gas_costs_clone = gas_costs.clone();
        let backend_tx_sender_clone = backend_tx_sender.clone();
        tokio::spawn(async move {
            match ws::handle_connection(
                stream,
                state_clone,
                broadcast_rx,
                broadcast_tx_clone,
                provider_clone,
                gas_costs_clone,
                contract_address,
                backend_tx_sender_clone,
            )
            .await
            {
                Ok(_) => tracing::info!("Connection from {} closed cleanly", addr),
                Err(e) => tracing::error!("Connection from {} error: {}", addr, e),
            }
        });
    }

    Ok(())
}
