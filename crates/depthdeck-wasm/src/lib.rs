#![forbid(unsafe_code)]

use depthdeck_core::{ReplaySession, TapeReader};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmReplay {
    inner: ReplaySession,
    duration_ns: u64,
    info: TapeInfo,
}

#[derive(Serialize)]
struct TapeInfo {
    metadata: TapeMetadataInfo,
    records: usize,
    chunks: usize,
    compression_ratio: f64,
    duration_ms: f64,
    inter_arrival_buckets: [u64; 5],
}

#[derive(Serialize)]
struct TapeMetadataInfo {
    exchange: String,
    symbol: String,
    depth: u16,
    price_precision: u8,
    quantity_precision: u8,
    capture_started_unix_ns: String,
    generator: String,
}

#[wasm_bindgen]
impl WasmReplay {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<WasmReplay, JsValue> {
        console_error_panic_hook::set_once();
        let reader = TapeReader::open(bytes).map_err(js_error)?;
        let duration_ns = reader.duration_ns();
        let mut inter_arrival_buckets = [0_u64; 5];
        for pair in reader.records().windows(2) {
            let delta = pair[1]
                .receive_offset_ns
                .saturating_sub(pair[0].receive_offset_ns);
            let bucket = match delta {
                0..100_000 => 0,
                100_000..1_000_000 => 1,
                1_000_000..10_000_000 => 2,
                10_000_000..100_000_000 => 3,
                _ => 4,
            };
            inter_arrival_buckets[bucket] += 1;
        }
        let metadata = reader.metadata();
        let info = TapeInfo {
            metadata: TapeMetadataInfo {
                exchange: metadata.exchange.clone(),
                symbol: metadata.symbol.clone(),
                depth: metadata.depth,
                price_precision: metadata.price_precision,
                quantity_precision: metadata.quantity_precision,
                capture_started_unix_ns: metadata.capture_started_unix_ns.to_string(),
                generator: metadata.generator.clone(),
            },
            records: reader.records().len(),
            chunks: reader.chunk_count(),
            compression_ratio: reader.compression_ratio(),
            duration_ms: duration_ns as f64 / 1_000_000.0,
            inter_arrival_buckets,
        };
        let inner = ReplaySession::new(reader).map_err(js_error)?;
        Ok(Self {
            inner,
            duration_ns,
            info,
        })
    }

    pub fn advance(&mut self, elapsed_ms: f64) -> Result<JsValue, JsValue> {
        let elapsed_ns = milliseconds_to_nanoseconds(elapsed_ms)?;
        let frame = self.inner.advance(elapsed_ns).map_err(js_error)?;
        serde_wasm_bindgen::to_value(&frame).map_err(js_error)
    }

    pub fn seek(&mut self, elapsed_ms: f64) -> Result<JsValue, JsValue> {
        let elapsed_ns = milliseconds_to_nanoseconds(elapsed_ms)?;
        let frame = self.inner.seek(elapsed_ns).map_err(js_error)?;
        serde_wasm_bindgen::to_value(&frame).map_err(js_error)
    }

    #[wasm_bindgen(getter)]
    pub fn duration_ms(&self) -> f64 {
        self.duration_ns as f64 / 1_000_000.0
    }

    pub fn digest(&self) -> String {
        self.inner.digest()
    }

    pub fn info(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.info).map_err(js_error)
    }
}

fn milliseconds_to_nanoseconds(value: f64) -> Result<u64, JsValue> {
    if !value.is_finite() || value < 0.0 || value > u64::MAX as f64 / 1_000_000.0 {
        return Err(JsValue::from_str("elapsed milliseconds are out of range"));
    }
    Ok((value * 1_000_000.0) as u64)
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
