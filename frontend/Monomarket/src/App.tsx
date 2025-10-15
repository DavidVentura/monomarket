import { useEffect, useRef, useState } from "react";
import { ethers } from "ethers";
import type {
  AppStatus,
  ClientMessage,
  GasInfo,
  InitialState,
  LogEntry,
  NeedsToRegister,
  ServerMessage,
  State,
  WaitingServerParams,
} from "./types";
import "./App.css";

const WS_URL = "ws://localhost:8090";
const CHAIN_ID = 30143n;
const RECONNECT_DELAY = 100;

const CONTRACT_ABI = [
  "function register() external",
  "function buy(uint256 amount) external",
  "function sell(uint256 amount) external",
];

const send = async (ws: WebSocket, data: ClientMessage) => {
  ws.send(JSON.stringify(data));
};

function App() {
  const [state, setState] = useState<State>({
    name: "InitialState",
    state: {},
  } satisfies InitialState);

  console.log(state);
  useEffect(() => {
    const privateKey = localStorage.getItem("wallet_privateKey");
    let loadedWallet: ethers.Wallet;

    if (privateKey) {
      loadedWallet = new ethers.Wallet(privateKey);
    } else {
      loadedWallet = new ethers.Wallet(ethers.Wallet.createRandom().privateKey);
      localStorage.setItem("wallet_privateKey", loadedWallet.privateKey);
    }

    setState({
      name: "WaitingServerParams",
      state: { wallet: loadedWallet },
    } satisfies WaitingServerParams);

    const ws = new WebSocket(WS_URL);

    ws.onopen = async () => {
      send(ws, { type: "get_nonce", address: loadedWallet.address });
    };

    ws.onmessage = async (event) => {
      const data: ServerMessage = JSON.parse(event.data);
      console.log(data);
      switch (data.type) {
        case "funded": {
          setState((prev) => {
            if (prev.name !== "WaitingServerParams") return prev;
            return {
              name: prev.name,
              state: { ...prev.state, funds: data.amount },
            };
          });
          break;
        }
        case "nonce_response": {
          setState((prev) => {
            if (prev.name !== "WaitingServerParams") return prev;
            return {
              name: prev.name,
              state: { ...prev.state, nonce: data.nonce },
            };
          });
          break;
        }
        case "connection_info": {
          setState((prev) => {
            if (prev.name !== "WaitingServerParams") return prev;
            const contract = new ethers.Contract(
              data.contract_address,
              CONTRACT_ABI,
              prev.state.wallet
            );

            return {
              name: prev.name,
              state: {
                ...prev.state,
                gasCosts: data.gas_costs,
                contract: contract,
              },
            };
          });
          break;
        }
      }
    };

    return () => {
      ws.close();
    };
  }, []);

  useEffect(() => {
    if (
      state.name === "WaitingServerParams" &&
      state.state.contract &&
      state.state.funds &&
      state.state.gasCosts &&
      state.state.nonce
    ) {
      console.log("Moving on from Waiting");
      setState((prev) => {
        if (
          prev.name === "WaitingServerParams" &&
          prev.state.contract &&
          prev.state.funds &&
          prev.state.gasCosts &&
          prev.state.nonce
        ) {
          return {
            state: prev.state as NeedsToRegister["state"],
            name: "NeedsToRegister",
          } satisfies NeedsToRegister;
        }
        return prev;
      });
    }
    console.log("new state", state);
  }, [state]);

  return (
    <div>
      {/* <pre>{JSON.stringify(state, null, 2)}</pre> */}
      {state.name === "NeedsToRegister" && <div>name pls</div>}
      {state.name === "WaitingServerParams" && <div>waiting for server</div>}
    </div>
  );
}

/*
function App() {
  const [registered, setRegistered] = useState(false);
  const [status, setStatus] = useState<AppStatus>("disconnected");
  const [wallet, setWallet] = useState<ethers.Wallet | null>(null);
  const [balance, setBalance] = useState<number>(0);
  const [nonce, setNonce] = useState<number | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [gasLimits, setGasLimits] = useState<GasInfo | null>(null);
  const [contract, setContract] = useState<ethers.Contract | null>(null);
  const [name, setName] = useState<string>("");

  const wsRef = useRef<WebSocket | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const reconnectTimeoutRef = useRef<number | null>(null);
  const walletRef = useRef<ethers.HDNodeWallet | ethers.Wallet | null>(null);

  useEffect(() => {
    if (registered) return;
    if (nonce == null) return;
    if (contract == null) return;
    if (wallet == null) return;
    console.log("nonce useffect");
    const run = async () => {
      console.log("nonce async");
      await sendTx({ txType: "register" }, contract, wallet);
      setRegistered(true);
      addLog(`sent register request`);
    };
    run();
  }, [nonce, registered, contract, wallet]);

  useEffect(() => {
    const privateKey = localStorage.getItem("wallet_privateKey");
    let loadedWallet: ethers.Wallet;

    if (privateKey) {
      loadedWallet = new ethers.Wallet(privateKey);
      addLog(`Loaded wallet: ${loadedWallet.address}`, "info");
    } else {
      loadedWallet = new ethers.Wallet(ethers.Wallet.createRandom().privateKey);
      localStorage.setItem("wallet_privateKey", loadedWallet.privateKey);
      addLog(`Created new wallet: ${loadedWallet.address}`, "info");
    }

    walletRef.current = loadedWallet;
    setWallet(loadedWallet);
    connectWebSocket();

    return () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      wsRef.current?.close();
    };
  }, []);

  const addLog = (message: string, logType: LogEntry["logType"] = "info") => {
    setLogs((prev) => [...prev, { timestamp: new Date(), message, logType }]);
  };

  const sendMessage = (msg: ClientMessage) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(msg));
    }
  };

  const connectWebSocket = () => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    addLog("Connecting to WebSocket...", "info");
    const ws = new WebSocket(WS_URL);
    wsRef.current = ws;

    ws.onopen = () => {
      addLog("Connected to server", "info");
      setStatus("connected");
    };

    ws.onmessage = async (event) => {
      try {
        const data: ServerMessage = JSON.parse(event.data);
        await handleServerMessage(data);
      } catch (e) {
        addLog(`Failed to parse message: ${e}`, "error");
      }
    };

    ws.onerror = () => {
      addLog("WebSocket error", "error");
    };

    ws.onclose = () => {
      addLog("Disconnected from server", "info");
      setStatus("disconnected");
      wsRef.current = null;

      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      reconnectTimeoutRef.current = window.setTimeout(() => {
        addLog("Reconnecting...", "info");
        connectWebSocket();
      }, RECONNECT_DELAY);
    };
  };

  const handleServerMessage = async (msg: ServerMessage) => {
    switch (msg.type) {
      case "connection_info":
        setContract(
          new ethers.Contract(msg.contract_address, CONTRACT_ABI, wallet)
        );
        setGasLimits(msg.gas_costs);
        addLog(`Connected to contract ${msg.contract_address}`, "info");
        addLog(
          `Gas costs - register: ${msg.gas_costs.register}, buy: ${msg.gas_costs.buy}, sell: ${msg.gas_costs.sell}`,
          "info"
        );
        break;

      case "price_update":
        addLog(
          `Price: ${msg.old_price} → ${msg.new_price} (block ${msg.block_number})`,
          "price"
        );
        break;

      case "bought":
        addLog(
          `${msg.name || msg.user} bought ${msg.amount} | Balance: ${
            msg.balance
          }, Holdings: ${msg.holdings}`,
          "bought"
        );
        break;

      case "sold":
        addLog(
          `${msg.name || msg.user} sold ${msg.amount} | Balance: ${
            msg.balance
          }, Holdings: ${msg.holdings}`,
          "sold"
        );
        break;

      case "name_set":
        addLog(`${msg.address} → ${msg.name}`, "name");
        break;

      case "position":
        addLog(
          `${msg.name || msg.address} | Cash: ${msg.balance}, Holdings: ${
            msg.holdings
          }, Price: ${msg.current_price}`,
          "info"
        );
        break;

      case "tx_error":
        addLog(`TX Error: ${msg.error}`, "error");
        break;

      case "nonce_response":
        const walletForNonce = walletRef.current;
        if (
          walletForNonce &&
          msg.address.toLowerCase() === walletForNonce.address.toLowerCase()
        ) {
          setNonce(msg.nonce);
          addLog(`Nonce received: ${msg.nonce}`, "info");
        } else {
          addLog(
            `Nonce response ignored: received=${msg.address}, wallet=${walletForNonce?.address}`,
            "error"
          );
        }
        break;

      case "funded":
        addLog(`Funded: ${msg.address} with ${msg.amount} wei`, "info");
        const currentWallet = walletRef.current;
        if (
          currentWallet &&
          msg.address.toLowerCase() === currentWallet.address.toLowerCase()
        ) {
          setBalance(msg.amount);
          setStatus("funded");
          sendMessage({ type: "get_nonce", address: currentWallet.address });
        } else {
          addLog(
            `Address mismatch: received=${msg.address}, wallet=${currentWallet?.address}`,
            "error"
          );
        }
        break;

      case "fund_error":
        addLog(`Fund error: ${msg.error}`, "error");
        break;

      case "tx_submitted":
        addLog(`TX submitted: ${msg.tx_hash}`, "info");
        break;
    }
  };

  const handleSetName = () => {
    const currentWallet = walletRef.current;
    if (!currentWallet) {
      addLog("Wallet not loaded", "error");
      return;
    }

    if (!name.trim()) {
      addLog("Please enter a name", "error");
      return;
    }

    addLog(`Setting name: ${name}`, "info");
    sendMessage({
      type: "set_name",
      name: name.trim(),
      address: currentWallet.address,
    });
  };

  type RawTx =
    | { txType: "buy" | "sell"; amount: number }
    | { txType: "register" };
  const sendTx = async (
    raw: RawTx,
    contract: ethers.Contract,
    wallet: ethers.Wallet
  ) => {
    if (nonce === null || !gasLimits) {
      addLog(`Wallet not ready ${wallet} ${nonce} ${gasLimits}`, "error");
      return;
    }

    const gasCost = gasLimits[raw.txType] * GAS_PRICE;
    if (balance < gasCost) {
      addLog(
        `Insufficient balance. Need ${gasCost} wei, have ${balance} wei`,
        "error"
      );
      return;
    }

    try {
      const tx =
        raw.txType === "register"
          ? await contract[raw.txType].populateTransaction()
          : await contract[raw.txType].populateTransaction(raw.amount);

      const fullTx = {
        ...tx,
        nonce,
        chainId: CHAIN_ID,
        gasPrice: GAS_PRICE,
        gasLimit: gasLimits[raw.txType],
      };

      const signedTx = await wallet.signTransaction(fullTx);
      sendMessage({ type: "raw_tx", raw_tx: signedTx });

      setNonce(nonce + 1);
      setBalance(balance - gasCost);
      if (raw.txType === "register") {
        addLog(`${raw.txType} transaction sent`);
      } else {
        addLog(
          `${raw.txType} transaction sent: ${raw.amount} shares`,
          raw.txType === "buy" ? "bought" : "sold"
        );
      }
    } catch (e) {
      addLog(`Failed to send ${raw.txType} transaction: ${e}`, "error");
    }
  };

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const canTrade = status === "funded" && gasLimits && nonce !== null;
  const buyGasCost = gasLimits ? gasLimits.buy * GAS_PRICE : 0;
  const sellGasCost = gasLimits ? gasLimits.sell * GAS_PRICE : 0;

  console.log(canTrade, status, gasLimits, nonce);
  return (
    <div className="app">
      <header>
        <h1>MonoMarket</h1>
        <div className={`status status-${status}`}>{status}</div>
      </header>

      <div className="info-section">
        <div className="wallet-info">
          <strong>Address:</strong> {wallet?.address || "Loading..."}
          <div className="balance-info"></div>
        </div>
        {status === "funded" && (
          <div className="balance-info">
            <strong>Balance:</strong>{" "}
            {Math.trunc(balance / 100_000_000_000_000).toString()} (~
            {Math.trunc(balance / buyGasCost)} txs left)
          </div>
        )}
      </div>

      {status === "connected" && (
        <div className="name-section">
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSetName()}
            placeholder="Enter your name"
            className="name-input"
            autoFocus
          />
          <button onClick={handleSetName} className="name-btn">
            Set Name
          </button>
        </div>
      )}

      {status === "funded" && !!contract && !!wallet && (
        <div className="trading-section">
          <button
            onClick={() =>
              sendTx({ txType: "buy", amount: 1 }, contract, wallet)
            }
            disabled={!canTrade || balance < buyGasCost}
            className="trade-btn buy-btn"
          >
            Buy 1
          </button>
          <button
            onClick={() => sendTx({ txType: "sell", amount: 1 })}
            disabled={!canTrade || balance < sellGasCost}
            className="trade-btn sell-btn"
          >
            Sell 1
          </button>
        </div>
      )}

      <div className="console-section">
        <h2>Console</h2>
        <div className="console">
          {logs.map((log, i) => (
            <div key={i} className={`log-entry log-${log.logType}`}>
              <span className="log-time">
                [{log.timestamp.toLocaleTimeString()}]
              </span>{" "}
              {log.message}
            </div>
          ))}
          <div ref={logsEndRef} />
        </div>
      </div>
    </div>
  );
}
*/
export default App;
