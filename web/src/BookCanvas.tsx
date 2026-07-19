import { useEffect, useRef } from "react";
import type { ReplayFrame } from "./types";

export function BookCanvas({ frame }: { frame: ReplayFrame | null }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const bounds = canvas.getBoundingClientRect();
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.floor(bounds.width * ratio));
    canvas.height = Math.max(1, Math.floor(bounds.height * ratio));
    const context = canvas.getContext("2d");
    if (!context) return;
    context.scale(ratio, ratio);
    context.clearRect(0, 0, bounds.width, bounds.height);
    context.font = "12px 'IBM Plex Mono', Consolas, monospace";
    context.textBaseline = "middle";
    context.fillStyle = "#738397";
    context.fillText("BIDS", 16, 18);
    context.fillText("ASKS", bounds.width / 2 + 16, 18);
    if (!frame?.bids.length || !frame.asks.length) {
      context.fillStyle = "#aab6c5";
      context.fillText("Waiting for a validated snapshot…", 16, 52);
      return;
    }
    const levels = 12;
    const rows = [...frame.bids.slice(0, levels), ...frame.asks.slice(0, levels)];
    const maximum = Math.max(...rows.map((level) => Number(level.quantity)), 1);
    drawSide(context, frame.bids.slice(0, levels), 0, bounds.width / 2, maximum, "#3df5a0");
    drawSide(context, frame.asks.slice(0, levels), bounds.width / 2, bounds.width / 2, maximum, "#ff6e87");
  }, [frame]);

  return <canvas ref={canvasRef} className="book-canvas" aria-label="Reconstructed bid and ask depth ladder" />;
}

function drawSide(
  context: CanvasRenderingContext2D,
  levels: ReplayFrame["bids"],
  x: number,
  width: number,
  maximum: number,
  color: string,
) {
  levels.forEach((level, index) => {
    const y = 40 + index * 25;
    const barWidth = (Number(level.quantity) / maximum) * (width - 24);
    context.globalAlpha = 0.12;
    context.fillStyle = color;
    context.fillRect(x + 8, y - 10, barWidth, 20);
    context.globalAlpha = 1;
    context.fillStyle = color;
    context.fillText(level.price, x + 16, y);
    context.fillStyle = "#c7d1dd";
    context.textAlign = "right";
    context.fillText(Number(level.quantity).toFixed(5), x + width - 16, y);
    context.textAlign = "left";
  });
}
