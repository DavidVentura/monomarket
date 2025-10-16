import { useEffect, useRef, useState } from "react";
import { ethers } from "ethers";
import classNames from "classnames";
import type {
  AwaitingRegistration,
  ClientMessage,
  InitialState,
  LogEntry,
  NeedsToRegister,
  PricePoint,
  ServerMessage,
  State,
  TradableState,
  WaitingServerParams,
} from "./types";
import { PriceChart } from "./PriceChart";
import "./App.css";

const WS_URL = "ws://localhost:8090";
const CHAIN_ID = 30143n;
const GAS_PRICE = 0x21d664903cn;

const CONTRACT_ABI = [
  "function register() external",
  "function buy(uint256 amount) external",
  "function sell(uint256 amount) external",
];

interface Portfolio {
  balance: number;
  holdings: number;
}

function App() {
  const [state, setState] = useState<State>({
    name: "InitialState",
    state: {},
  } satisfies InitialState);

  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [nameInput, setNameInput] = useState<string>("");
  const [currentPortfolio, setCurrentPortfolio] = useState<
    Map<string, Portfolio>
  >(new Map());
  const [names, setNames] = useState<Map<string, string>>(new Map());

  const wsRef = useRef<WebSocket | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);

  const addLog = (message: string, logType: LogEntry["logType"] = "info") => {
    setLogs((prev) => [...prev, { timestamp: new Date(), message, logType }]);
  };

  const sendMessage = (msg: ClientMessage) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(msg));
    }
  };

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

    setState({
      name: "WaitingServerParams",
      state: { wallet: loadedWallet },
    } satisfies WaitingServerParams);

    const ws = new WebSocket(WS_URL);
    wsRef.current = ws;

    ws.onopen = async () => {
      addLog("Connected to server", "info");
      sendMessage({ type: "get_nonce", address: loadedWallet.address });
    };

    ws.onmessage = async (event) => {
      const data: ServerMessage = JSON.parse(event.data);
      console.log("Server message:", data);

      switch (data.type) {
        case "funded": {
          addLog(`Funded: ${data.amount} wei`, "info");
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
          addLog(`Nonce received: ${data.nonce}`, "info");
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
          addLog(`Connected to contract: ${data.contract_address}`, "info");
          addLog(
            `Gas costs - register: ${data.gas_costs.register}, buy: ${data.gas_costs.buy}, sell: ${data.gas_costs.sell}`,
            "info"
          );
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

        case "price_update": {
          addLog(
            `Price: ${data.new_price} (block ${data.block_number})`,
            "price"
          );
          setState((prev) => {
            if (prev.name === "WaitingServerParams") {
              return {
                name: prev.name,
                state: { ...prev.state, currentPrice: data.new_price },
              };
            }
            if (prev.name === "NeedsToRegister") {
              return {
                name: prev.name,
                state: { ...prev.state, currentPrice: data.new_price },
              };
            }
            if (prev.name === "TradableState") {
              const newPricePoint: PricePoint = {
                blockNumber: data.block_number,
                price: data.new_price,
                timestamp: new Date(),
              };
              const updatedHistory = [...prev.state.priceHistory, newPricePoint];
              const limitedHistory = updatedHistory.slice(-200);

              return {
                name: prev.name,
                state: {
                  ...prev.state,
                  currentPrice: data.new_price,
                  priceHistory: limitedHistory,
                },
              };
            }
            return prev;
          });
          break;
        }

        case "current_price": {
          addLog(`Current price: ${data.price}`, "price");
          setState((prev) => {
            if (prev.name === "WaitingServerParams") {
              return {
                name: prev.name,
                state: { ...prev.state, currentPrice: data.price },
              };
            }
            if (prev.name === "NeedsToRegister") {
              return {
                name: prev.name,
                state: { ...prev.state, currentPrice: data.price },
              };
            }
            if (prev.name === "TradableState") {
              return {
                name: prev.name,
                state: { ...prev.state, currentPrice: data.price },
              };
            }
            return prev;
          });
          break;
        }

        case "name_set": {
          addLog(`${data.address} → ${data.name}`, "name");
          setNames((prev) =>
            new Map(prev).set(data.address.toLowerCase(), data.name)
          );
          break;
        }

        case "position": {
          addLog(
            `${data.address} | Cash: ${data.balance}, Holdings: ${data.holdings}`,
            "info"
          );

          setCurrentPortfolio((prev) =>
            new Map(prev).set(data.address.toLowerCase(), {
              balance: data.balance,
              holdings: data.holdings,
            })
          );

          setState((prev) => {
            const isOurAddress =
              data.address.toLowerCase() ===
              (prev.name !== "InitialState"
                ? prev.state.wallet.address.toLowerCase()
                : null);

            if (!isOurAddress) return prev;

            if (prev.name === "WaitingServerParams") {
              return {
                name: prev.name,
                state: {
                  ...prev.state,
                  balance: data.balance,
                  holdings: data.holdings,
                },
              };
            }
            if (prev.name === "NeedsToRegister") {
              return {
                name: prev.name,
                state: {
                  ...prev.state,
                  balance: data.balance,
                  holdings: data.holdings,
                },
              };
            }
            if (prev.name === "AwaitingRegistration") {
              addLog("Registration confirmed!", "info");
              const initialPricePoint: PricePoint = {
                blockNumber: data.block_number,
                price: prev.state.currentPrice,
                timestamp: new Date(),
              };
              return {
                name: "TradableState",
                state: {
                  ...prev.state,
                  balance: data.balance,
                  holdings: data.holdings,
                  priceHistory: [initialPricePoint],
                },
              } satisfies TradableState;
            }
            if (prev.name === "TradableState") {
              return {
                name: prev.name,
                state: {
                  ...prev.state,
                  balance: data.balance,
                  holdings: data.holdings,
                },
              };
            }
            return prev;
          });
          break;
        }

        case "tx_error": {
          addLog(`TX Error: ${data.error}`, "error");
          break;
        }

        case "tx_submitted": {
          addLog(`TX submitted: ${data.tx_hash}`, "info");
          break;
        }
      }
    };

    ws.onerror = () => {
      addLog("WebSocket error", "error");
    };

    ws.onclose = () => {
      addLog("Disconnected from server", "info");
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
      state.state.nonce !== undefined &&
      state.state.currentPrice !== undefined
    ) {
      console.log("Moving to NeedsToRegister");
      setState({
        state: {
          wallet: state.state.wallet,
          funds: state.state.funds,
          contract: state.state.contract,
          gasCosts: state.state.gasCosts,
          nonce: state.state.nonce,
          currentPrice: state.state.currentPrice,
          balance: state.state.balance,
          holdings: state.state.holdings,
        },
        name: "NeedsToRegister",
      } satisfies NeedsToRegister);
    }
  }, [state]);

  const handleSetName = async () => {
    if (state.name !== "NeedsToRegister") return;
    if (!nameInput.trim()) {
      addLog("Please enter a name", "error");
      return;
    }

    const name = nameInput.trim();
    addLog(`Setting name: ${name}`, "info");

    sendMessage({
      type: "set_name",
      name: name,
      address: state.state.wallet.address,
    });

    const {
      wallet,
      funds,
      contract,
      gasCosts,
      nonce,
      balance,
      holdings,
      currentPrice,
    } = state.state;
    const isAlreadyRegistered =
      balance !== undefined &&
      holdings !== undefined &&
      (balance > 0 || holdings > 0);

    if (isAlreadyRegistered) {
      addLog("Already registered, moving to trading", "info");
      const initialPricePoint: PricePoint = {
        blockNumber: 0,
        price: currentPrice,
        timestamp: new Date(),
      };
      setState({
        name: "TradableState",
        state: {
          wallet,
          funds,
          contract,
          gasCosts,
          nonce,
          name,
          currentPrice,
          balance: balance!,
          holdings: holdings!,
          priceHistory: [initialPricePoint],
        },
      } satisfies TradableState);
      return;
    }

    try {
      const tx = await contract.register.populateTransaction();
      const fullTx = {
        ...tx,
        nonce,
        chainId: CHAIN_ID,
        gasPrice: GAS_PRICE,
        gasLimit: gasCosts.register,
      };

      const signedTx = await wallet.signTransaction(fullTx);
      sendMessage({ type: "raw_tx", raw_tx: signedTx });

      addLog("Register transaction sent", "info");

      const gasCost = gasCosts.register * Number(GAS_PRICE);

      setState({
        name: "AwaitingRegistration",
        state: {
          wallet,
          funds: funds - gasCost,
          contract,
          gasCosts,
          nonce: nonce + 1,
          name,
          balance,
          holdings,
          currentPrice,
        },
      } satisfies AwaitingRegistration);
    } catch (e) {
      addLog(`Failed to send register transaction: ${e}`, "error");
    }
  };

  type RawTx =
    | { txType: "buy" | "sell"; amount: number }
    | { txType: "register" };

  const sendTx = async (raw: RawTx) => {
    if (state.name !== "TradableState") return;

    const { wallet, contract, gasCosts, nonce, funds } = state.state;

    const gasCost = gasCosts[raw.txType] * Number(GAS_PRICE);
    if (funds < gasCost) {
      addLog(
        `Insufficient balance. Need ${gasCost} wei, have ${funds} wei`,
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
        gasLimit: gasCosts[raw.txType],
      };

      const signedTx = await wallet.signTransaction(fullTx);
      sendMessage({ type: "raw_tx", raw_tx: signedTx });

      setState((prev) => {
        if (prev.name !== "TradableState") return prev;
        return {
          name: prev.name,
          state: {
            ...prev.state,
            nonce: prev.state.nonce + 1,
            funds: prev.state.funds - gasCost,
          },
        };
      });

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

  return (
    <div className="app">
      <header>
        <h1>MonoMarket</h1>
        <div className={`status status-${state.name}`}>{state.name}</div>
      </header>

      <div className="info-section">
        <div className="wallet-info">
          <strong>Address:</strong>{" "}
          {state.name !== "InitialState"
            ? state.state.wallet?.address
            : "Loading..."}
        </div>
        {state.name === "TradableState" && (
          <div className="balance-info">
            <strong>Balance:</strong> {state.state.balance} credits
            <br />
            <strong>Holdings:</strong> {state.state.holdings} shares
            <br />
            <strong>Price:</strong> {state.state.currentPrice}
            <br />
            <strong>Funds:</strong>{" "}
            {Math.trunc(state.state.funds / 100_000_000_000_000).toString()} (~
            {Math.trunc(
              state.state.funds / (state.state.gasCosts.buy * Number(GAS_PRICE))
            )}{" "}
            txs left)
          </div>
        )}
      </div>

      {state.name === "WaitingServerParams" && (
        <div className="loading-section">
          <div className="loading-spinner">
            <svg width="80" height="80" viewBox="0 0 80 80">
              <circle
                cx="40"
                cy="40"
                r="32"
                fill="none"
                stroke="#569cd6"
                strokeWidth="4"
                strokeLinecap="round"
                strokeDasharray="150 50"
              >
                <animateTransform
                  attributeName="transform"
                  type="rotate"
                  from="0 40 40"
                  to="360 40 40"
                  dur="1.5s"
                  repeatCount="indefinite"
                />
              </circle>
              <circle
                cx="40"
                cy="40"
                r="24"
                fill="none"
                stroke="#4ec9b0"
                strokeWidth="3"
                strokeLinecap="round"
                strokeDasharray="100 50"
              >
                <animateTransform
                  attributeName="transform"
                  type="rotate"
                  from="360 40 40"
                  to="0 40 40"
                  dur="2s"
                  repeatCount="indefinite"
                />
              </circle>
            </svg>
          </div>
          <p className="loading-text">Funding your wallet...</p>
        </div>
      )}

      {state.name === "NeedsToRegister" && (
        <div className="name-section">
          <input
            type="text"
            value={nameInput}
            onChange={(e) => setNameInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSetName()}
            placeholder="Enter your name"
            className="name-input"
            autoFocus
          />
          <button onClick={handleSetName} className="name-btn">
            Register
          </button>
        </div>
      )}

      {state.name === "AwaitingRegistration" && (
        <div className="name-section">
          <p>Registering as {state.state.name}...</p>
        </div>
      )}

      {state.name === "TradableState" && (
        <>
          {state.state.priceHistory.length >= 2 && (
            <div className="chart-section">
              <h2>Price Chart</h2>
              <PriceChart priceHistory={state.state.priceHistory} />
            </div>
          )}
          <div className="trading-section">
            <button
              onClick={() => sendTx({ txType: "buy", amount: 1 })}
              disabled={
                state.state.funds <
                  state.state.gasCosts.buy * Number(GAS_PRICE) ||
                state.state.currentPrice > state.state.balance
              }
              className="trade-btn buy-btn"
            >
              Buy 1
            </button>
            <button
              onClick={() => sendTx({ txType: "sell", amount: 1 })}
              disabled={
                state.state.funds <
                  state.state.gasCosts.sell * Number(GAS_PRICE) ||
                state.state.holdings < 1
              }
              className="trade-btn sell-btn"
            >
              Sell 1
            </button>
          </div>
        </>
      )}

      {state.name !== "InitialState" && currentPortfolio.size > 0 && (
        <div className="portfolio-section">
          <h2>All Players</h2>
          <table className="portfolio-table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Balance</th>
                <th>Holdings</th>
                <th>Net Worth</th>
              </tr>
            </thead>
            <tbody>
              {Array.from(currentPortfolio.entries())
                .map(([address, portfolio]) => {
                  const price = state.state.currentPrice || 50;
                  const netWorth =
                    portfolio.balance + portfolio.holdings * price;
                  return { address, portfolio, netWorth };
                })
                .sort((a, b) => b.netWorth - a.netWorth)
                .map(({ address, portfolio, netWorth }) => {
                  const isUser =
                    state.state.wallet.address.toLowerCase() === address;
                  return (
                    <tr
                      key={address}
                      className={classNames({ "user-row": isUser })}
                    >
                      <td>{names.get(address) || "Unknown"}</td>
                      <td>{portfolio.balance}</td>
                      <td>{portfolio.holdings}</td>
                      <td>{netWorth}</td>
                    </tr>
                  );
                })}
            </tbody>
          </table>
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

export default App;
