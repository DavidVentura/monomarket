use alloy::{
    network::{EthereumWallet, TransactionBuilder},
    primitives::{Address, Bytes, TxHash, U256},
    providers::{Provider, ProviderBuilder, WalletProvider, WsConnect},
    rpc::types::{Filter, Log, TransactionRequest},
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolEvent,
    transports::Transport,
};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{RwLock, broadcast},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    StockMarket,
    "contract/out/StockMarket.sol/StockMarket.json"
);

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    SetName { name: String, address: String },
    RawTx { raw_tx: String },
    GetNonce { address: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
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
struct GasInfo {
    register: u64,
    buy: u64,
    sell: u64,
}

struct AppState {
    names: HashMap<Address, String>,
    seen_logs: HashSet<(TxHash, u64)>,
    current_price: u64,
    balances: HashMap<Address, u64>,
    holdings: HashMap<Address, u64>,
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
struct GasCosts {
    register: u64,
    buy: u64,
    sell: u64,
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
    let buy_gas = 60_000; // TODO: 42k
    let sell_gas = 60_000; // TODO: 42k

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

    let state_clone = state.clone();
    let broadcast_tx_clone = broadcast_tx.clone();
    let provider_write_clone = provider_write.clone();
    let gas_costs_clone = gas_costs.clone();
    tokio::spawn(async move {
        if let Err(e) = run_websocket_server(
            &ws_port,
            state_clone,
            broadcast_tx_clone,
            provider_write_clone,
            gas_costs_clone,
            contract_addr,
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
    let mut stream = sub.into_stream();

    tracing::info!("Subscribed to contract logs (monadLogs) and blocks!");

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

async fn run_websocket_server<T, P>(
    port: &str,
    state: Arc<RwLock<AppState>>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
    provider: P,
    gas_costs: Arc<GasCosts>,
    contract_address: Address,
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
        tokio::spawn(async move {
            match handle_connection(
                stream,
                state_clone,
                broadcast_rx,
                broadcast_tx_clone,
                provider_clone,
                gas_costs_clone,
                contract_address,
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

async fn handle_connection<T, P>(
    stream: TcpStream,
    state: Arc<RwLock<AppState>>,
    mut broadcast_rx: broadcast::Receiver<ServerMessage>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
    provider: P,
    gas_costs: Arc<GasCosts>,
    contract_address: Address,
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
                    Ok(client_msg) => {
                        match client_msg {
                            ClientMessage::SetName { name, address } => {
                                match address.parse::<Address>() {
                                    Ok(addr) => {
                                        tracing::info!("Setting name: {} → {}", address, name);

                                        match provider.get_balance(addr).await {
                                            Ok(balance) => {
                                                tracing::info!(
                                                    "Balance for {}: {} wei",
                                                    address,
                                                    balance
                                                );

                                                if balance == U256::ZERO {
                                                    tracing::info!(
                                                        "Balance is zero, funding account..."
                                                    );

                                                    let backend_addr =
                                                        provider.default_signer_address();
                                                    tracing::info!(
                                                        "Backend wallet address: {:?}",
                                                        backend_addr
                                                    );

                                                    match provider.get_balance(backend_addr).await {
                                                        Ok(backend_balance) => {
                                                            tracing::info!(
                                                                "Backend wallet balance: {} wei",
                                                                backend_balance
                                                            );
                                                        }
                                                        Err(e) => {
                                                            tracing::error!(
                                                                "Failed to get backend balance: {}",
                                                                e
                                                            );
                                                        }
                                                    }

                                                    let funding_amount =
                                                        U256::from(500_000_000_000_000_000u64); // 0.5MON .. ~50 clicks? lil bit small

                                                    tracing::info!(
                                                        "Funding {} with {} wei (0.2 MON)",
                                                        address,
                                                        funding_amount
                                                    );

                                                    let gas_price = U256::from(0x21d664903cu64);
                                                    let gas_limit = 100000u64;
                                                    tracing::info!(
                                                        "Gas cost for funding tx: {} wei",
                                                        gas_price * U256::from(gas_limit)
                                                    );

                                                    let tx = TransactionRequest::default()
                                                        .to(addr)
                                                        .value(funding_amount)
                                                        .with_gas_limit(gas_limit)
                                                        .with_gas_price(gas_price.to::<u128>());

                                                    match provider.send_transaction(tx).await {
                                                        Ok(pending) => {
                                                            let tx_hash = *pending.tx_hash();
                                                            tracing::info!(
                                                                "📤 Funding tx sent: {:?}",
                                                                tx_hash
                                                            );

                                                            match pending.get_receipt().await {
                                                                Ok(receipt) => {
                                                                    tracing::info!(
                                                                        "✅ Funding tx confirmed: {:?} (block: {}, status: {})",
                                                                        tx_hash,
                                                                        receipt
                                                                            .block_number
                                                                            .unwrap_or_default(),
                                                                        if receipt.status() {
                                                                            "success"
                                                                        } else {
                                                                            "failed"
                                                                        }
                                                                    );

                                                                    if !receipt.status() {
                                                                        let error_msg = format!(
                                                                            "Funding transaction failed: {:?}",
                                                                            tx_hash
                                                                        );
                                                                        tracing::error!(
                                                                            "{}", error_msg
                                                                        );
                                                                        let msg = ServerMessage::TxError {
                                                                            error: error_msg,
                                                                        };
                                                                        let _ =
                                                                            broadcast_tx.send(msg);
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    let error_msg = format!(
                                                                        "Failed to get funding tx receipt: {}",
                                                                        e
                                                                    );
                                                                    tracing::error!(
                                                                        "{}", error_msg
                                                                    );
                                                                    let msg =
                                                                        ServerMessage::TxError {
                                                                            error: error_msg,
                                                                        };
                                                                    let _ = broadcast_tx.send(msg);
                                                                }
                                                            }
                                                        }
                                                        Err(e) => {
                                                            let error_msg = format!(
                                                                "Failed to fund {}: {}",
                                                                address, e
                                                            );
                                                            tracing::error!("{}", error_msg);

                                                            let msg = ServerMessage::TxError {
                                                                error: error_msg,
                                                            };
                                                            let _ = broadcast_tx.send(msg);
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Failed to check balance for {}: {}",
                                                    address,
                                                    e
                                                );
                                            }
                                        }

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
                                        tracing::error!(
                                            "Failed to parse address '{}': {}",
                                            address,
                                            e
                                        );
                                    }
                                }
                            }
                            ClientMessage::RawTx { raw_tx } => {
                                tracing::info!(
                                    "Received raw tx: {}...",
                                    &raw_tx[..20.min(raw_tx.len())]
                                );

                                match raw_tx.parse::<Bytes>() {
                                    Ok(bytes) => {
                                        match provider.send_raw_transaction(&bytes).await {
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

                                                let msg =
                                                    ServerMessage::TxError { error: error_msg };
                                                let _ = broadcast_tx.send(msg);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let error_msg =
                                            format!("Failed to parse transaction: {}", e);
                                        tracing::error!("{}", error_msg);

                                        let msg = ServerMessage::TxError { error: error_msg };
                                        let _ = broadcast_tx.send(msg);
                                    }
                                }
                            }
                            ClientMessage::GetNonce { address } => {
                                match address.parse::<Address>() {
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
                                                let error_msg =
                                                    format!("Failed to get nonce: {}", e);
                                                tracing::error!("{}", error_msg);

                                                let msg =
                                                    ServerMessage::TxError { error: error_msg };
                                                let _ = broadcast_tx.send(msg);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to parse address '{}': {}",
                                            address,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
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
