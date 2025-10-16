
interface GasBarProps {
  funds: number;
  gasCostBuy: number;
  gasPrice: bigint;
}

export function GasBar({ funds, gasCostBuy, gasPrice }: GasBarProps) {
  const FULL_THRESHOLD = 0.5e18;
  const EMPTY_THRESHOLD = Number(gasPrice) * gasCostBuy;

  const percentage = Math.max(
    0,
    Math.min(100, ((funds - EMPTY_THRESHOLD) / (FULL_THRESHOLD - EMPTY_THRESHOLD)) * 100)
  );

  const hue = (percentage / 100) * 120;
  const color = `hsl(${hue}, 70%, 60%)`;

  return (
    <div className="gas-bar-container">
      <div className="gas-bar">
        <div
          className="gas-bar-fill"
          style={{
            height: `${percentage}%`,
            backgroundColor: color,
            transition: "all 0.3s ease",
          }}
        />
      </div>
      <div className="gas-bar-label">Gas</div>
    </div>
  );
}
