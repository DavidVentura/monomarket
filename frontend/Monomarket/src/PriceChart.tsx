import { useEffect, useRef } from "react";
import type { PricePoint } from "./types";

interface PriceChartProps {
  priceHistory: PricePoint[];
}

export function PriceChart({ priceHistory }: PriceChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrollOffsetRef = useRef(0);
  const animationFrameRef = useRef<number | undefined>(undefined);
  const lastHistoryLengthRef = useRef(0);
  const newPointProgressRef = useRef(1);
  const isAnimatingRef = useRef(false);

  const VISIBLE_POINTS = 35;
  const NEW_POINT_ANIMATION_FRAMES = 10;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const targetOffset = Math.max(0, priceHistory.length - VISIBLE_POINTS);

    if (priceHistory.length > lastHistoryLengthRef.current) {
      newPointProgressRef.current = 0;
      lastHistoryLengthRef.current = priceHistory.length;
      if (priceHistory.length > VISIBLE_POINTS) {
        scrollOffsetRef.current = targetOffset - 1;
      }
    }

    if (isAnimatingRef.current) {
      return;
    }

    isAnimatingRef.current = true;

    const animate = () => {
      if (newPointProgressRef.current < 1) {
        newPointProgressRef.current += 1 / NEW_POINT_ANIMATION_FRAMES;
        newPointProgressRef.current = Math.min(1, newPointProgressRef.current);
      }

      const diff = targetOffset - scrollOffsetRef.current;
      const shouldContinue =
        Math.abs(diff) > 0.01 || newPointProgressRef.current < 1;

      if (Math.abs(diff) > 0.01) {
        scrollOffsetRef.current += diff * 0.15;
        scrollOffsetRef.current = Math.max(0, scrollOffsetRef.current);
      } else {
        scrollOffsetRef.current = targetOffset;
      }

      const ctx = canvas.getContext("2d");
      if (!ctx) {
        isAnimatingRef.current = false;
        return;
      }

      const dpr = window.devicePixelRatio || 1;
      const rect = canvas.getBoundingClientRect();

      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;

      ctx.scale(dpr, dpr);

      const width = rect.width;
      const height = rect.height;

      const padding = { top: 20, right: 40, bottom: 30, left: 50 };
      const chartWidth = width - padding.left - padding.right;
      const chartHeight = height - padding.top - padding.bottom;

      ctx.fillStyle = "#1e1e1e";
      ctx.fillRect(0, 0, width, height);

      const minPrice = 0;
      const maxPrice = 100;

      ctx.strokeStyle = "#3e3e42";
      ctx.lineWidth = 1;

      for (let i = 0; i <= 10; i++) {
        const y = padding.top + (chartHeight * i) / 10;
        ctx.beginPath();
        ctx.moveTo(padding.left, y);
        ctx.lineTo(padding.left + chartWidth, y);
        ctx.stroke();

        const price = maxPrice - (i * (maxPrice - minPrice)) / 10;
        ctx.fillStyle = "#858585";
        ctx.font = "12px Consolas, Monaco, monospace";
        ctx.textAlign = "right";
        ctx.textBaseline = "middle";
        ctx.fillText(price.toFixed(0), padding.left - 10, y);
      }

      const numTicks = Math.min(10, VISIBLE_POINTS);
      const tickSpacing = chartWidth / (numTicks - 1);
      for (let i = 0; i < numTicks; i++) {
        const x = padding.left + i * tickSpacing;
        ctx.beginPath();
        ctx.moveTo(x, padding.top + chartHeight);
        ctx.lineTo(x, padding.top + chartHeight + 5);
        ctx.stroke();
      }

      const xScale = chartWidth / Math.max(1, VISIBLE_POINTS - 1);
      const yScale = chartHeight / (maxPrice - minPrice);

      const startIndex = Math.floor(scrollOffsetRef.current);
      const endIndex = Math.min(
        startIndex + VISIBLE_POINTS,
        priceHistory.length
      );

      ctx.save();
      ctx.beginPath();
      ctx.rect(padding.left, padding.top, chartWidth, chartHeight);
      ctx.clip();

      for (let i = startIndex; i < endIndex - 1; i++) {
        const current = priceHistory[i];
        const next = priceHistory[i + 1];

        const localIndex1 = i - scrollOffsetRef.current;
        const localIndex2 = i + 1 - scrollOffsetRef.current;

        if (localIndex1 < VISIBLE_POINTS && localIndex2 > 0) {
          const x1 = padding.left + localIndex1 * xScale;
          const y1 =
            padding.top + chartHeight - (current.price - minPrice) * yScale;
          const x2 = padding.left + localIndex2 * xScale;
          const y2 =
            padding.top + chartHeight - (next.price - minPrice) * yScale;

          const isNewest = i === priceHistory.length - 2;
          const progress = isNewest ? newPointProgressRef.current : 1;

          const isIncrease = next.price >= current.price;
          ctx.strokeStyle = isIncrease ? "#4ec9b0" : "#f48771";
          ctx.lineWidth = 2;

          ctx.beginPath();
          ctx.moveTo(x1, y1);
          const interpolatedX = x1 + (x2 - x1) * progress;
          const interpolatedY = y1 + (y2 - y1) * progress;
          ctx.lineTo(interpolatedX, interpolatedY);
          ctx.stroke();
        }
      }

      ctx.restore();

      if (priceHistory.length > 1 && endIndex > startIndex) {
        const lastVisiblePoint = priceHistory[endIndex - 1];
        const localIndex = endIndex - 1 - scrollOffsetRef.current;

        if (localIndex >= 0 && localIndex < VISIBLE_POINTS) {
          const x = padding.left + localIndex * xScale;
          const y =
            padding.top +
            chartHeight -
            (lastVisiblePoint.price - minPrice) * yScale;

          ctx.fillStyle = "#569cd6";
          ctx.beginPath();
          ctx.arc(x, y, 4, 0, 2 * Math.PI);
          ctx.fill();
        }
      }

      if (shouldContinue) {
        animationFrameRef.current = requestAnimationFrame(animate);
      } else {
        isAnimatingRef.current = false;
      }
    };

    animationFrameRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
        animationFrameRef.current = undefined;
      }
      isAnimatingRef.current = false;
    };
  }, [priceHistory]);

  return (
    <canvas
      ref={canvasRef}
      className="price-chart"
      style={{ width: "100%", height: "300px" }}
    />
  );
}
