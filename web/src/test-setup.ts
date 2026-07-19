import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(cleanup);

class MockWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  postMessage(message: { type: string }) {
    if (message.type !== "load") return;
    queueMicrotask(() => this.onmessage?.(new MessageEvent("message", {
      data: {
        type: "loaded",
        info: {
          metadata: {
            exchange: "synthetic-kraken-spot-v2",
            symbol: "BTC/USD",
            depth: 100,
            price_precision: 1,
            quantity_precision: 8,
            capture_started_unix_ns: "1753012800000000000",
            generator: "depthdeck-test",
          },
          records: 10,
          chunks: 1,
          compression_ratio: 0.2,
          duration_ms: 1_000,
          inter_arrival_buckets: [0, 0, 9, 0, 0],
        },
        frame: {
          elapsed_ns: 1_000_000,
          exchange_timestamp: "2026-07-19T12:00:00Z",
          bids: [{ price: "100.0", quantity: "1.00000000" }],
          asks: [{ price: "101.0", quantity: "2.00000000" }],
          spread: "1.0",
          checksum_valid: true,
          synchronized: true,
          messages: 1,
          price_level_mutations: 2,
          last_record_kind: "Snapshot",
          last_payload: "{}",
        },
        digest: "abcd1234",
      },
    })));
  }
  terminate() {}
}

Object.defineProperty(globalThis, "Worker", { value: MockWorker, configurable: true });
Object.defineProperty(globalThis, "fetch", {
  value: async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(8) }),
  configurable: true,
});
Object.defineProperty(globalThis, "requestAnimationFrame", {
  value: () => 1,
  configurable: true,
});
Object.defineProperty(globalThis, "cancelAnimationFrame", {
  value: () => undefined,
  configurable: true,
});
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  value: () => null,
  configurable: true,
});
