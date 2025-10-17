mod backend;
mod chain_events;
mod ws;
mod ws_axum;

use alloy::{
    network::EthereumWallet,
    primitives::{Address, TxHash},
    providers::{Provider, ProviderBuilder, WalletProvider, WsConnect},
    rpc::types::{Filter, Log},
    signers::local::PrivateKeySigner,
    transports::Transport,
};
use anyhow::Result;
use axum::{
    extract::{State as AxumState, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::StreamExt;
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tokio::sync::{RwLock, broadcast, mpsc};
use tower_http::services::ServeDir;
use ws::ServerMessage;

#[derive(Debug)]
pub enum BackendTxEvent {
    Fund(Address, mpsc::Sender<ServerMessage>),
    Tick,
    GameOver,
}

pub struct AppState {
    pub names: HashMap<Address, String>,
    pub seen_logs: HashSet<(TxHash, u64)>,
    pub current_price: u64,
    pub balances: HashMap<Address, u64>,
    pub holdings: HashMap<Address, u64>,
    pub backend_nonce: u64,
    pub last_position_block: u64,
    pub game_start_block: Option<u64>,
    pub game_end_block: Option<u64>,
    pub current_block_height: u64,
}

impl AppState {
    fn new() -> Self {
        Self {
            names: HashMap::new(),
            seen_logs: HashSet::new(),
            current_price: 50,
            balances: HashMap::new(),
            holdings: HashMap::new(),
            backend_nonce: 0,
            last_position_block: 0,
            game_start_block: None,
            game_end_block: None,
            current_block_height: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GasCosts {
    pub register: u64,
    pub buy: u64,
    pub sell: u64,
}

#[derive(Clone)]
struct ServerState<T: Transport + Clone, P: Provider<T> + WalletProvider + Clone + 'static> {
    app_state: Arc<RwLock<AppState>>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
    provider: P,
    gas_costs: Arc<GasCosts>,
    contract_address: Address,
    backend_tx_sender: mpsc::Sender<BackendTxEvent>,
    _phantom: std::marker::PhantomData<T>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let rpc_url = env::var("RPC_URL").expect("RPC_URL not set");
    let contract_address = env::var("CONTRACT_ADDRESS").expect("CONTRACT_ADDRESS not set");
    let private_key = env::var("PRIVATE_KEY").expect("PRIVATE_KEY not set");
    let ws_port = env::var("WEBSOCKET_PORT").unwrap_or_else(|_| "8000".to_string());

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
    let buy_gas = 39733;
    let sell_gas = 39733;

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

    tracing::info!("Querying contract for game state...");
    let contract = backend::StockMarket::new(contract_addr, &provider_write);
    let start_block = contract.startBlock().call().await?._0;
    let end_block = contract.endBlock().call().await?._0;

    if start_block > 0 {
        tracing::info!(
            "Game state: started at block {}, ends at block {}",
            start_block,
            end_block
        );
        let mut state_guard = state.write().await;
        state_guard.game_start_block = Some(start_block.to());
        state_guard.game_end_block = Some(end_block.to());
    } else {
        tracing::info!("Game not started yet");
    }

    let backend_address = provider_write.default_signer_address();
    let backend_nonce = provider_write
        .get_transaction_count(backend_address)
        .await?;
    tracing::info!(
        "Backend wallet {:?} starting nonce: {}",
        backend_address,
        backend_nonce
    );
    {
        let mut state_guard = state.write().await;
        state_guard.backend_nonce = backend_nonce;
    }

    let (broadcast_tx, _) = broadcast::channel::<ServerMessage>(1000);
    let (backend_tx_sender, backend_tx_receiver) = mpsc::channel::<BackendTxEvent>(100);

    let provider_write_clone = provider_write.clone();
    let broadcast_tx_clone = broadcast_tx.clone();
    let state_clone_backend = state.clone();
    tokio::spawn(async move {
        if let Err(e) = backend::backend_tx_executor(
            backend_tx_receiver,
            provider_write_clone,
            contract_addr,
            broadcast_tx_clone,
            state_clone_backend,
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
        if let Err(e) = run_http_server(
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
            tracing::error!("HTTP server error: {}", e);
        }
    });

    tracing::info!("Starting block subscriber...");

    let block_sub = provider_read.subscribe_blocks().await?;
    let mut block_stream = block_sub.into_stream();

    let state_clone_blocks = state.clone();
    let backend_tx_sender_clone_blocks = backend_tx_sender.clone();
    tokio::spawn(async move {
        let mut last_ended_block = 0u64;
        while let Some(block) = block_stream.next().await {
            let block_number = block.number;
            tracing::info!(
                "🧱 New Block: {} (timestamp: {})",
                block_number,
                block.timestamp
            );

            {
                let mut state_guard = state_clone_blocks.write().await;
                state_guard.current_block_height = block_number;
            }

            let game_end_block = {
                let state_guard = state_clone_blocks.read().await;
                state_guard.game_end_block
            };

            if let Some(ends_at) = game_end_block {
                if ends_at > last_ended_block && block_number >= ends_at {
                    last_ended_block = ends_at;
                    let _ = backend_tx_sender_clone_blocks
                        .send(BackendTxEvent::GameOver)
                        .await;
                }
                if ends_at > block_number {
                    tracing::info!("⏰ Auto-tick triggered on block {}", block_number);
                    let _ = backend_tx_sender_clone_blocks
                        .send(BackendTxEvent::Tick)
                        .await;
                }
            }
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

async fn run_http_server<T, P>(
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
    let server_state = ServerState {
        app_state: state,
        broadcast_tx,
        provider,
        gas_costs,
        contract_address,
        backend_tx_sender,
        _phantom: std::marker::PhantomData,
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new("frontend/Monomarket/dist"))
        .with_state(server_state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("HTTP server listening on {}", addr);
    tracing::info!("  WebSocket endpoint: ws://{}/ws", addr);
    tracing::info!("  Static files from: frontend/Monomarket/dist/");

    axum::serve(listener, app).await?;

    Ok(())
}

async fn ws_handler<T, P>(
    ws: WebSocketUpgrade,
    AxumState(state): AxumState<ServerState<T, P>>,
) -> impl IntoResponse
where
    T: Transport + Clone,
    P: Provider<T> + WalletProvider + Clone + 'static,
{
    ws.on_upgrade(move |socket| async move {
        let broadcast_rx = state.broadcast_tx.subscribe();

        if let Err(e) = ws_axum::handle_axum_connection(
            socket,
            state.app_state,
            broadcast_rx,
            state.broadcast_tx,
            state.provider,
            state.gas_costs,
            state.contract_address,
            state.backend_tx_sender,
        )
        .await
        {
            tracing::error!("WebSocket connection error: {}", e);
        }
    })
}
