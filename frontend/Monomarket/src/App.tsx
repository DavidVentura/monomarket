import { useEffect, useRef, useState } from "react";
import { ethers } from "ethers";
import classNames from "classnames";
import type {
  AwaitingRegistration,
  ClientMessage,
  GameEnded,
  InitialState,
  LogEntry,
  NeedsToRegister,
  PricePoint,
  ServerMessage,
  State,
  TradableState,
  WaitingForGameStart,
  WaitingServerParams,
} from "./types";
import { PriceChart } from "./PriceChart";
import { GasBar } from "./GasBar";
import { FloatingMessage } from "./FloatingMessage";
import { SpinningCoin } from "./SpinningCoin";
import "./App.css";

const WS_URL = (() => {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/ws`;
})();
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
  const [floatingMessages, setFloatingMessages] = useState<
    Array<{ id: number; x: number; y: number; message: string }>
  >([]);

  const wsRef = useRef<WebSocket | null>(null);
  const logsEndRef = useRef<HTMLDivElement>(null);
  const consoleRef = useRef<HTMLDivElement>(null);
  const prevHoldingsRef = useRef<Map<string, number>>(new Map());
  const messageIdRef = useRef(0);
  const namesRef = useRef<Map<string, string>>(new Map());
  const stateNameRef = useRef<State["name"]>("InitialState");

  const addLog = (message: string, logType: LogEntry["logType"] = "info") => {
    setLogs((prev) => [...prev, { timestamp: new Date(), message, logType }]);
  };

  const sendMessage = (msg: ClientMessage) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(msg));
    }
  };

  const isAdmin = () => {
    return document.cookie
      .split(";")
      .some((item) => item.trim().startsWith("ADMIN=1"));
  };

  const handleRestartGame = () => {
    sendMessage({ type: "restart_game" });
    addLog("Restart game requested", "info");
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
          console.log(`Funded: ${data.amount} wei`);
          setState((prev) => {
            if (prev.name === "InitialState") return prev;
            return {
              ...prev,
              state: { ...prev.state, funds: data.amount },
            } as State;
          });
          break;
        }

        case "nonce_response": {
          console.log(`Nonce received: ${data.nonce}`);
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
          console.log(`Connected to contract: ${data.contract_address}`);
          console.log(
            `Gas costs - register: ${data.gas_costs.register}, buy: ${data.gas_costs.buy}, sell: ${data.gas_costs.sell}`
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
          setState((prev) => {
            switch (prev.name) {
              case "WaitingServerParams":
              case "NeedsToRegister":
              case "WaitingForGameStart":
                return {
                  ...prev,
                  state: {
                    ...prev.state,
                    currentPrice: data.new_price,
                    currentBlockHeight: data.block_number,
                  },
                } as State;

              case "TradableState": {
                const newPricePoint: PricePoint = {
                  blockNumber: data.block_number,
                  price: data.new_price,
                  timestamp: new Date(),
                };
                const updatedHistory = [
                  ...prev.state.priceHistory,
                  newPricePoint,
                ];
                const limitedHistory = updatedHistory.slice(-200);

                const firstBlockTimestamp =
                  prev.state.firstBlockTimestamp || new Date();

                return {
                  ...prev,
                  state: {
                    ...prev.state,
                    currentPrice: data.new_price,
                    currentBlockHeight: data.block_number,
                    priceHistory: limitedHistory,
                    firstBlockTimestamp,
                  },
                } as State;
              }

              default:
                return prev;
            }
          });
          break;
        }

        case "current_price": {
          setState((prev) => {
            switch (prev.name) {
              case "WaitingServerParams":
              case "NeedsToRegister":
              case "WaitingForGameStart":
              case "TradableState":
                return {
                  ...prev,
                  state: {
                    ...prev.state,
                    currentPrice: data.price,
                  },
                } as State;
              default:
                return prev;
            }
          });
          break;
        }

        case "current_block_height": {
          setState((prev) => {
            switch (prev.name) {
              case "WaitingServerParams":
              case "NeedsToRegister":
              case "AwaitingRegistration":
              case "WaitingForGameStart":
              case "TradableState":
                return {
                  ...prev,
                  state: {
                    ...prev.state,
                    currentBlockHeight: data.height,
                  },
                } as State;
              default:
                return prev;
            }
          });
          break;
        }

        case "name_set": {
          addLog(`${data.name} joined`, "info");
          const addressLower = data.address.toLowerCase();
          namesRef.current.set(addressLower, data.name);
          setNames((prev) => new Map(prev).set(addressLower, data.name));
          break;
        }

        case "position": {
          const addressLower = data.address.toLowerCase();
          const previousHoldings = prevHoldingsRef.current.get(addressLower);
          const playerName = namesRef.current.get(addressLower) || "Unknown";

          console.log(
            "position",
            stateNameRef.current,
            "previous",
            previousHoldings
          );
          if (
            previousHoldings !== undefined &&
            stateNameRef.current === "TradableState"
          ) {
            const holdingsDiff = data.holdings - previousHoldings;
            if (holdingsDiff > 0) {
              addLog(`${playerName} bought`, "info");
            } else if (holdingsDiff < 0) {
              addLog(`${playerName} sold`, "info");
            }
            console.log(
              `Position update for ${playerName}: ${previousHoldings} → ${data.holdings} (diff: ${holdingsDiff})`
            );
          }

          prevHoldingsRef.current.set(addressLower, data.holdings);

          setCurrentPortfolio((prev) =>
            new Map(prev).set(addressLower, {
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

            switch (prev.name) {
              case "WaitingServerParams":
              case "NeedsToRegister":
              case "TradableState":
                return {
                  ...prev,
                  state: {
                    ...prev.state,
                    balance: data.balance,
                    holdings: data.holdings,
                  },
                } as State;

              case "AwaitingRegistration":
                return {
                  name: "WaitingForGameStart",
                  state: {
                    ...prev.state,
                    balance: data.balance,
                    holdings: data.holdings,
                  },
                };

              default:
                return prev;
            }
          });
          break;
        }

        case "tx_error": {
          addLog(`TX Error: ${data.error}`, "error");
          break;
        }

        case "tx_submitted": {
          console.log(`TX submitted: ${data.tx_hash}`);
          break;
        }

        case "game_started": {
          console.log(
            `Game started: blocks ${data.start_height} to ${data.end_height}, state = ${stateNameRef.current}`
          );
          addLog(`Game started`, "info");
          setState((prev) => {
            if (
              prev.name === "WaitingServerParams" ||
              prev.name === "NeedsToRegister" ||
              prev.name === "AwaitingRegistration" ||
              prev.name === "WaitingForGameStart" ||
              prev.name === "TradableState"
            ) {
              return {
                ...prev,
                state: {
                  ...prev.state,
                  startHeight: data.start_height,
                  endHeight: data.end_height,
                },
              } as State;
            }
            if (prev.name === "GameEnded") {
              // this is intentionally pulled out of state before spreading
              // eslint-disable-next-line @typescript-eslint/no-unused-vars
              const { firstBlockTimestamp, priceHistory, ...restState } =
                prev.state;
              return {
                name: "WaitingForGameStart",
                state: {
                  ...restState,
                  startHeight: data.start_height,
                  endHeight: data.end_height,
                  currentBlockHeight: undefined,
                },
              } satisfies WaitingForGameStart;
            }
            return prev;
          });
          break;
        }

        case "game_ended": {
          console.log("Game ended");
          addLog("Game ended", "info");
          setState((prev) => {
            if (prev.name === "TradableState") {
              return {
                name: "GameEnded",
                state: prev.state,
              } satisfies GameEnded;
            }
            return prev;
          });
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
        name: "NeedsToRegister",
        state: {
          ...state.state,
          funds: state.state.funds,
          contract: state.state.contract,
          gasCosts: state.state.gasCosts,
          nonce: state.state.nonce,
          currentPrice: state.state.currentPrice,
        },
      } satisfies NeedsToRegister);
    }
  }, [state]);

  useEffect(() => {
    console.log("state", state.name);
    if (
      state.name === "WaitingForGameStart" &&
      state.state.startHeight !== undefined &&
      state.state.endHeight !== undefined &&
      state.state.currentBlockHeight !== undefined
    ) {
      console.log("Moving to TradableState");
      const initialPricePoint: PricePoint = {
        blockNumber: state.state.startHeight,
        price: state.state.currentPrice,
        timestamp: new Date(),
      };
      setState({
        name: "TradableState",
        state: {
          ...state.state,
          startHeight: state.state.startHeight,
          endHeight: state.state.endHeight,
          currentBlockHeight: state.state.currentBlockHeight,
          priceHistory: [initialPricePoint],
          firstBlockTimestamp: undefined,
        },
      } satisfies TradableState);
    }
  }, [state]);

  const handleSetName = async () => {
    if (state.name !== "NeedsToRegister") return;
    if (!nameInput.trim()) {
      addLog("Please enter a name", "error");
      return;
    }

    const name = nameInput.trim();
    console.log(`Setting name: ${name}`);

    sendMessage({
      type: "set_name",
      name: name,
      address: state.state.wallet.address,
    });

    const { wallet, funds, contract, gasCosts, nonce, balance, holdings } =
      state.state;
    const isAlreadyRegistered =
      balance !== undefined &&
      holdings !== undefined &&
      (balance > 0 || holdings > 0);

    if (isAlreadyRegistered) {
      console.log("Already registered, waiting for game to start");
      console.log("Current state.state:", state.state);
      setState({
        name: "WaitingForGameStart",
        state: { ...state.state, name, balance, holdings },
      } satisfies WaitingForGameStart);
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
          ...state.state,
          name,
          funds: funds - gasCost,
          nonce: nonce + 1,
          balance,
          holdings,
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

      if (raw.txType !== "register") {
        console.log(`${raw.txType} transaction sent: ${raw.amount} shares`);
      }
    } catch (e) {
      addLog(`Failed to send ${raw.txType} transaction: ${e}`, "error");
    }
  };

  useEffect(() => {
    stateNameRef.current = state.name;
  }, [state.name]);

  useEffect(() => {
    if (consoleRef.current) {
      consoleRef.current.scrollTop = consoleRef.current.scrollHeight;
    }
  }, [logs]);

  return (
    <div className="app">
      <div className="floating-messages-layer">
        {floatingMessages.map((msg) => (
          <FloatingMessage
            key={msg.id}
            x={msg.x}
            y={msg.y}
            message={msg.message}
            onComplete={() => {
              setFloatingMessages((prev) =>
                prev.filter((m) => m.id !== msg.id)
              );
            }}
          />
        ))}
      </div>
      <header>
        <h1>MonoMarket</h1>
      </header>

      {state.name === "WaitingServerParams" && (
        <div className="loading-section">
          <SpinningCoin size={180} />
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
            maxLength={16}
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

      {state.name === "WaitingForGameStart" && (
        <div className="loading-section">
          <SpinningCoin size={180} />
          <p className="loading-text">Waiting for game to start...</p>
          {isAdmin() && (
            <button onClick={handleRestartGame} className="admin-restart-btn">
              Restart Game
            </button>
          )}
        </div>
      )}

      {state.name === "GameEnded" && (
        <div className="loading-section">
          <p className="loading-text">Game Over</p>
          <p className="winner-text">
            Winner:{" "}
            {(() => {
              const sorted = Array.from(currentPortfolio.entries())
                .map(([address, portfolio]) => {
                  const price = state.state.currentPrice;
                  const netWorth =
                    portfolio.balance + portfolio.holdings * price;
                  return { address, netWorth };
                })
                .sort((a, b) => b.netWorth - a.netWorth);

              if (sorted.length === 0) return "Unknown";
              const winnerAddress = sorted[0].address;
              return names.get(winnerAddress) || "Unknown";
            })()}
          </p>
          {isAdmin() && (
            <button onClick={handleRestartGame} className="admin-restart-btn">
              Restart Game
            </button>
          )}
        </div>
      )}

      {(state.name === "TradableState" || state.name === "GameEnded") && (
        <>
          {state.state.priceHistory.length >= 1 && (
            <div className="chart-section">
              <div className="stat-cards">
                <div className="stat-card">
                  <div className="stat-label">Balance</div>
                  <div className="stat-value">{state.state.balance}</div>
                  <div className="stat-unit">credits</div>
                </div>
                <div className="stat-card">
                  <div className="stat-label">Holdings</div>
                  <div className="stat-value">{state.state.holdings}</div>
                  <div className="stat-unit">shares</div>
                </div>
                <div className="stat-card">
                  <div className="stat-label">Net Worth</div>
                  <div className="stat-value">
                    {state.state.balance +
                      state.state.holdings * state.state.currentPrice}
                  </div>
                  <div className="stat-unit">total</div>
                </div>
                {state.name === "TradableState" && (
                  <div className="stat-card stat-card-timer">
                    <div className="stat-label">Time Remaining</div>
                    <div
                      className="stat-value"
                      style={{
                        color: (() => {
                          const blocksRemaining = Math.max(
                            0,
                            state.state.endHeight -
                              state.state.currentBlockHeight
                          );

                          if (!state.state.firstBlockTimestamp) {
                            return "#858585";
                          }

                          const elapsedMs =
                            new Date().getTime() -
                            state.state.firstBlockTimestamp.getTime();
                          const elapsedSeconds = elapsedMs / 1000;
                          const blocksPassed =
                            state.state.currentBlockHeight -
                            state.state.startHeight;

                          if (blocksPassed < 10 || elapsedSeconds <= 0) {
                            return "#858585";
                          }

                          const secondsPerBlock = elapsedSeconds / blocksPassed;
                          const estimatedSecondsRemaining =
                            blocksRemaining * secondsPerBlock;

                          const ratio = Math.min(
                            1,
                            Math.max(0, estimatedSecondsRemaining / 90)
                          );
                          const hue = ratio * 120;

                          return `hsl(${hue}, 70%, 60%)`;
                        })(),
                      }}
                    >
                      {(() => {
                        const blocksRemaining = Math.max(
                          0,
                          state.state.endHeight - state.state.currentBlockHeight
                        );

                        if (!state.state.firstBlockTimestamp) {
                          return "--";
                        }

                        const elapsedMs =
                          new Date().getTime() -
                          state.state.firstBlockTimestamp.getTime();
                        const elapsedSeconds = elapsedMs / 1000;
                        const blocksPassed =
                          state.state.currentBlockHeight -
                          state.state.startHeight;

                        if (blocksPassed < 10 || elapsedSeconds <= 0) {
                          return "--";
                        }

                        const secondsPerBlock = elapsedSeconds / blocksPassed;
                        const estimatedSecondsRemaining = Math.round(
                          blocksRemaining * secondsPerBlock
                        );

                        if (estimatedSecondsRemaining < 60) {
                          return `${estimatedSecondsRemaining}s`;
                        } else {
                          const minutes = Math.floor(
                            estimatedSecondsRemaining / 60
                          );
                          const seconds = estimatedSecondsRemaining % 60;
                          return `${minutes}m ${seconds}s`;
                        }
                      })()}
                    </div>
                    <div className="stat-unit">
                      {Math.max(
                        0,
                        state.state.endHeight - state.state.currentBlockHeight
                      )}{" "}
                      blocks
                    </div>
                  </div>
                )}
              </div>

              <div className="chart-with-gas">
                <GasBar
                  funds={state.state.funds}
                  gasCostBuy={state.state.gasCosts.buy}
                  gasPrice={GAS_PRICE}
                />
                <PriceChart priceHistory={state.state.priceHistory} />
              </div>
            </div>
          )}
          <div className="trading-section">
            <button
              onClick={(e) => {
                const isGameOver =
                  state.state.priceHistory.length >= 2 &&
                  state.state.priceHistory[state.state.priceHistory.length - 1]
                    .blockNumber >= state.state.endHeight;
                const isOutOfGas =
                  state.state.funds <
                  state.state.gasCosts.buy * Number(GAS_PRICE);
                const isOutOfBalance =
                  state.state.currentPrice > state.state.balance;
                if (isGameOver || isOutOfGas || isOutOfBalance) {
                  const messageId = messageIdRef.current++;
                  const message = isGameOver
                    ? "game over"
                    : isOutOfGas
                    ? "out of gas"
                    : "not enough balance";
                  setFloatingMessages((prev) => [
                    ...prev,
                    {
                      id: messageId,
                      x: e.clientX,
                      y: e.clientY,
                      message,
                    },
                  ]);
                  setTimeout(() => {
                    setFloatingMessages((prev) =>
                      prev.filter((msg) => msg.id !== messageId)
                    );
                  }, 1300);
                } else {
                  sendTx({ txType: "buy", amount: 1 });
                }
              }}
              className={`trade-btn buy-btn ${
                (state.state.priceHistory.length >= 2 &&
                  state.state.priceHistory[state.state.priceHistory.length - 1]
                    .blockNumber >= state.state.endHeight) ||
                state.state.funds <
                  state.state.gasCosts.buy * Number(GAS_PRICE) ||
                state.state.currentPrice > state.state.balance
                  ? "disabled"
                  : ""
              }`}
            >
              Buy
            </button>
            <button
              onClick={(e) => {
                const isGameOver =
                  state.state.priceHistory.length >= 2 &&
                  state.state.priceHistory[state.state.priceHistory.length - 1]
                    .blockNumber >= state.state.endHeight;
                const isOutOfGas =
                  state.state.funds <
                  state.state.gasCosts.sell * Number(GAS_PRICE);
                const isNoStocks = state.state.holdings < 1;
                if (isGameOver || isOutOfGas || isNoStocks) {
                  const messageId = messageIdRef.current++;
                  const message = isGameOver
                    ? "game over"
                    : isOutOfGas
                    ? "out of gas"
                    : "no stocks";
                  setFloatingMessages((prev) => [
                    ...prev,
                    {
                      id: messageId,
                      x: e.clientX,
                      y: e.clientY,
                      message,
                    },
                  ]);
                  setTimeout(() => {
                    setFloatingMessages((prev) =>
                      prev.filter((msg) => msg.id !== messageId)
                    );
                  }, 1300);
                } else {
                  sendTx({ txType: "sell", amount: 1 });
                }
              }}
              className={`trade-btn sell-btn ${
                (state.state.priceHistory.length >= 2 &&
                  state.state.priceHistory[state.state.priceHistory.length - 1]
                    .blockNumber >= state.state.endHeight) ||
                state.state.funds <
                  state.state.gasCosts.sell * Number(GAS_PRICE) ||
                state.state.holdings < 1
                  ? "disabled"
                  : ""
              }`}
            >
              Sell
            </button>
          </div>
        </>
      )}

      {state.name !== "InitialState" && currentPortfolio.size > 0 && (
        <div className="portfolio-section">
          <h2>Scores</h2>
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
                  const price =
                    state.state.currentPrice === undefined
                      ? 50
                      : state.state.currentPrice;
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
        <div className="console" ref={consoleRef}>
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
