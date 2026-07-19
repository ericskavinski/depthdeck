export interface BookLevel {
  price: string;
  quantity: string;
}

export interface ReplayFrame {
  elapsed_ns: number;
  exchange_timestamp: string | null;
  bids: BookLevel[];
  asks: BookLevel[];
  spread: string | null;
  checksum_valid: boolean;
  synchronized: boolean;
  messages: number;
  price_level_mutations: number;
  last_record_kind: string | null;
  last_payload: string | null;
}

export interface TapeInfo {
  metadata: {
    exchange: string;
    symbol: string;
    depth: number;
    price_precision: number;
    quantity_precision: number;
    capture_started_unix_ns: string;
    generator: string;
  };
  records: number;
  chunks: number;
  compression_ratio: number;
  duration_ms: number;
  inter_arrival_buckets: number[];
}

export type WorkerRequest =
  | { type: "load"; bytes: ArrayBuffer }
  | { type: "advance"; elapsedMs: number }
  | { type: "seek"; elapsedMs: number };

export type WorkerResponse =
  | { type: "loaded"; info: TapeInfo; frame: ReplayFrame; digest: string }
  | { type: "frame"; frame: ReplayFrame; digest: string }
  | { type: "error"; message: string };
