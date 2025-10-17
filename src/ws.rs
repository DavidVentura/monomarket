use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    SetName { name: String, address: String },
    RawTx { raw_tx: String },
    GetNonce { address: String },
    RestartGame,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    ConnectionInfo {
        contract_address: String,
        gas_costs: GasInfo,
    },
    PriceUpdate {
        new_price: u64,
        block_number: u64,
    },
    CurrentPrice {
        price: u64,
    },
    NameSet {
        address: String,
        name: String,
    },
    Position {
        address: String,
        balance: u64,
        holdings: u64,
        block_number: u64,
    },
    TxError {
        error: String,
    },
    NonceResponse {
        address: String,
        nonce: u64,
    },
    Funded {
        address: String,
        amount: u64,
    },
    FundError {
        address: String,
        error: String,
    },
    TxSubmitted {
        tx_hash: String,
    },
    GameStarted {
        start_height: u64,
        end_height: u64,
    },
    GameEnded,
    CurrentBlockHeight {
        height: u64,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct GasInfo {
    pub register: u64,
    pub buy: u64,
    pub sell: u64,
}
