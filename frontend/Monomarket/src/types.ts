import { ethers } from "ethers";

export interface GasInfo {
  register: number;
  buy: number;
  sell: number;
}

export type ServerMessage =
  | { type: "funded"; address: string; amount: number }
  | { type: "fund_error"; address: string; error: string }
  | { type: "connection_info"; contract_address: string; gas_costs: GasInfo }
  | { type: "nonce_response"; address: string; nonce: number }
  | {
      type: "price_update";
      new_price: number;
      block_number: number;
    }
  | { type: "current_price"; price: number }
  | { type: "name_set"; address: string; name: string }
  | {
      type: "position";
      address: string;
      balance: number;
      holdings: number;
      block_number: number;
    }
  | { type: "tx_error"; error: string }
  | { type: "tx_submitted"; tx_hash: string };

export type ClientMessage =
  | { type: "set_name"; name: string; address: string }
  | { type: "raw_tx"; raw_tx: string }
  | { type: "get_nonce"; address: string };

export type AppStatus = "disconnected" | "connected" | "funded";

export type State =
  | InitialState
  | WaitingServerParams
  | NeedsToRegister
  | AwaitingRegistration
  | TradableState;

export type InitialState = {
  name: "InitialState";
  state: {
    wallet?: ethers.Wallet;
  };
};

export type WaitingServerParams = {
  name: "WaitingServerParams";
  state: {
    wallet: ethers.Wallet;
    funds?: number;
    contract?: ethers.Contract;
    gasCosts?: GasInfo;
    nonce?: number;
    balance?: number;
    holdings?: number;
    currentPrice?: number;
  };
};

export type NeedsToRegister = {
  name: "NeedsToRegister";
  state: {
    wallet: ethers.Wallet;
    funds: number;
    contract: ethers.Contract;
    gasCosts: GasInfo;
    nonce: number;
    balance?: number;
    holdings?: number;
    currentPrice: number;
  };
};

export type AwaitingRegistration = {
  name: "AwaitingRegistration";
  state: {
    wallet: ethers.Wallet;
    funds: number;
    contract: ethers.Contract;
    gasCosts: GasInfo;
    nonce: number;
    name: string;
    balance?: number;
    holdings?: number;
    currentPrice: number;
  };
};

export type TradableState = {
  name: "TradableState";
  state: {
    wallet: ethers.Wallet;
    funds: number;
    contract: ethers.Contract;
    gasCosts: GasInfo;
    nonce: number;
    name: string;
    currentPrice: number;
    balance: number;
    holdings: number;
  };
};

export interface LogEntry {
  timestamp: Date;
  message: string;
  logType: "info" | "price" | "bought" | "sold" | "name" | "error";
}
