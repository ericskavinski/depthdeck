/// <reference lib="webworker" />

import init, { WasmReplay } from "./wasm/depthdeck_wasm.js";
import type { WorkerRequest, WorkerResponse } from "./types";

let replay: WasmReplay | undefined;
let initialized: Promise<unknown> | undefined;

self.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  try {
    initialized ??= init();
    await initialized;
    const request = event.data;
    if (request.type === "load") {
      replay?.free();
      replay = new WasmReplay(new Uint8Array(request.bytes));
      const frame = replay.seek(Math.min(1, replay.duration_ms));
      send({ type: "loaded", info: replay.info(), frame, digest: replay.digest() });
      return;
    }
    if (!replay) throw new Error("load a DepthDeck tape before replaying");
    const frame =
      request.type === "seek"
        ? replay.seek(request.elapsedMs)
        : replay.advance(request.elapsedMs);
    send({ type: "frame", frame, digest: replay.digest() });
  } catch (error) {
    send({ type: "error", message: error instanceof Error ? error.message : String(error) });
  }
};

function send(message: WorkerResponse) {
  self.postMessage(message);
}
