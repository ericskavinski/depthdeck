use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{RecordKind, TapeMetadata, TapeReader, TapeRecord, kraken_checksum};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: String,
    pub quantity: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayFrame {
    pub elapsed_ns: u64,
    pub exchange_timestamp: Option<String>,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub spread: Option<String>,
    pub checksum_valid: bool,
    pub synchronized: bool,
    pub messages: u64,
    pub price_level_mutations: u64,
    pub last_record_kind: Option<RecordKind>,
    pub last_payload: Option<String>,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("invalid Kraken book message at {offset_ns}ns: {message}")]
    InvalidMessage { offset_ns: u64, message: String },
    #[error("Kraken checksum mismatch at {offset_ns}ns: expected {expected}, calculated {actual}")]
    ChecksumMismatch {
        offset_ns: u64,
        expected: u32,
        actual: u32,
    },
}

#[derive(Debug, Clone)]
struct Level {
    price_atoms: i64,
    quantity_atoms: i64,
    price: Arc<str>,
    quantity: Arc<str>,
}

#[derive(Debug, Clone)]
struct OrderBook {
    depth: usize,
    price_precision: u8,
    quantity_precision: u8,
    bids: Vec<Level>,
    asks: Vec<Level>,
}

impl OrderBook {
    fn new(depth: usize, price_precision: u8, quantity_precision: u8) -> Self {
        Self {
            depth,
            price_precision,
            quantity_precision,
            bids: Vec::with_capacity(depth),
            asks: Vec::with_capacity(depth),
        }
    }

    fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
    }

    fn apply_side(
        levels: &mut Vec<Level>,
        updates: &[WireLevel],
        price_precision: u8,
        quantity_precision: u8,
    ) -> Result<u64, String> {
        let mut mutations = 0;
        for update in updates {
            let price_atoms = parse_scaled(decimal_text(&update.price)?, price_precision)?;
            let quantity_atoms = parse_scaled(decimal_text(&update.quantity)?, quantity_precision)?;
            let existing = levels
                .iter()
                .position(|level| level.price_atoms == price_atoms);
            if quantity_atoms == 0 {
                if let Some(index) = existing {
                    levels.remove(index);
                }
            } else {
                let level = Level {
                    price_atoms,
                    quantity_atoms,
                    price: Arc::from(format_scaled(price_atoms, price_precision)),
                    quantity: Arc::from(format_scaled(quantity_atoms, quantity_precision)),
                };
                if let Some(index) = existing {
                    levels[index] = level;
                } else {
                    levels.push(level);
                }
            }
            mutations += 1;
        }
        Ok(mutations)
    }

    fn sort_and_truncate(&mut self) {
        self.asks.sort_unstable_by_key(|level| level.price_atoms);
        self.bids
            .sort_unstable_by_key(|level| std::cmp::Reverse(level.price_atoms));
        self.asks.truncate(self.depth);
        self.bids.truncate(self.depth);
    }

    fn checksum(&self) -> u32 {
        let asks: Vec<_> = self
            .asks
            .iter()
            .take(10)
            .map(|level| (level.price.as_ref(), level.quantity.as_ref()))
            .collect();
        let bids: Vec<_> = self
            .bids
            .iter()
            .take(10)
            .map(|level| (level.price.as_ref(), level.quantity.as_ref()))
            .collect();
        kraken_checksum(&asks, &bids)
    }
}

pub struct ReplaySession {
    metadata: TapeMetadata,
    records: Vec<Arc<TapeRecord>>,
    cursor: usize,
    elapsed_ns: u64,
    book: OrderBook,
    exchange_timestamp: Option<String>,
    checksum_valid: bool,
    synchronized: bool,
    messages: u64,
    price_level_mutations: u64,
    last_record_kind: Option<RecordKind>,
    last_payload: Option<String>,
}

impl ReplaySession {
    pub fn new(reader: TapeReader) -> Result<Self, ReplayError> {
        let metadata = reader.metadata().clone();
        let records = reader.records().to_vec();
        Self::build(metadata, records)
    }

    pub fn live(metadata: TapeMetadata) -> Result<Self, ReplayError> {
        Self::build(metadata, Vec::new())
    }

    fn build(metadata: TapeMetadata, records: Vec<TapeRecord>) -> Result<Self, ReplayError> {
        Ok(Self {
            book: OrderBook::new(
                metadata.depth as usize,
                metadata.price_precision,
                metadata.quantity_precision,
            ),
            metadata,
            records: records.into_iter().map(Arc::new).collect(),
            cursor: 0,
            elapsed_ns: 0,
            exchange_timestamp: None,
            checksum_valid: false,
            synchronized: false,
            messages: 0,
            price_level_mutations: 0,
            last_record_kind: None,
            last_payload: None,
        })
    }

    pub fn advance(&mut self, elapsed_ns: u64) -> Result<ReplayFrame, ReplayError> {
        if elapsed_ns < self.elapsed_ns {
            return self.seek(elapsed_ns);
        }
        let mut last_applied = None;
        while let Some(record) = self.records.get(self.cursor).cloned() {
            if record.receive_offset_ns > elapsed_ns {
                break;
            }
            self.process_record(&record)?;
            self.cursor += 1;
            self.elapsed_ns = record.receive_offset_ns;
            last_applied = Some(record);
        }
        if let Some(record) = last_applied {
            self.remember(&record);
        }
        self.elapsed_ns = elapsed_ns;
        Ok(self.frame())
    }

    pub fn seek(&mut self, elapsed_ns: u64) -> Result<ReplayFrame, ReplayError> {
        self.cursor = 0;
        self.elapsed_ns = 0;
        self.book.clear();
        self.exchange_timestamp = None;
        self.checksum_valid = false;
        self.synchronized = false;
        self.messages = 0;
        self.price_level_mutations = 0;
        self.last_record_kind = None;
        self.last_payload = None;
        self.advance(elapsed_ns)
    }

    pub fn digest(&self) -> String {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&self.messages.to_le_bytes());
        hasher.update(&self.price_level_mutations.to_le_bytes());
        for level in self.book.asks.iter().chain(&self.book.bids) {
            hasher.update(&level.price_atoms.to_le_bytes());
            hasher.update(&level.quantity_atoms.to_le_bytes());
        }
        format!("{:08x}", hasher.finalize())
    }

    pub fn duration_ns(&self) -> u64 {
        self.records
            .iter()
            .last()
            .map_or(0, |record| record.receive_offset_ns)
    }

    pub fn apply_record(&mut self, record: &TapeRecord) -> Result<ReplayFrame, ReplayError> {
        self.process_record(record)?;
        self.elapsed_ns = record.receive_offset_ns;
        self.remember(record);
        Ok(self.frame())
    }

    fn process_record(&mut self, record: &TapeRecord) -> Result<(), ReplayError> {
        match record.kind {
            RecordKind::ConnectionOpened => {}
            RecordKind::ConnectionLost | RecordKind::ChecksumMismatch => {
                self.book.clear();
                self.synchronized = false;
                self.checksum_valid = false;
            }
            RecordKind::Snapshot | RecordKind::Update => self.apply_wire_record(record)?,
        }
        Ok(())
    }

    fn apply_wire_record(&mut self, record: &crate::TapeRecord) -> Result<(), ReplayError> {
        let payload: WireMessage = serde_json::from_slice(&record.payload).map_err(|error| {
            ReplayError::InvalidMessage {
                offset_ns: record.receive_offset_ns,
                message: error.to_string(),
            }
        })?;
        let message_type = payload.message_type.as_str();
        let expected_type = match record.kind {
            RecordKind::Snapshot => "snapshot",
            RecordKind::Update => "update",
            _ => unreachable!(),
        };
        if message_type != expected_type {
            return Err(invalid(record, "record kind does not match payload type"));
        }
        let data = payload
            .data
            .first()
            .ok_or_else(|| invalid(record, "missing book data"))?;
        if data.symbol != self.metadata.symbol {
            return Err(invalid(record, "symbol does not match tape metadata"));
        }
        let expected_checksum = data.checksum;

        let mut next = self.book.clone();
        if record.kind == RecordKind::Snapshot {
            next.clear();
        } else if !self.synchronized {
            return Err(invalid(record, "update received before a valid snapshot"));
        }
        let mutations = OrderBook::apply_side(
            &mut next.bids,
            &data.bids,
            next.price_precision,
            next.quantity_precision,
        )
        .and_then(|bid_mutations| {
            OrderBook::apply_side(
                &mut next.asks,
                &data.asks,
                next.price_precision,
                next.quantity_precision,
            )
            .map(|ask_mutations| bid_mutations + ask_mutations)
        })
        .map_err(|message| ReplayError::InvalidMessage {
            offset_ns: record.receive_offset_ns,
            message,
        })?;
        next.sort_and_truncate();
        let actual_checksum = next.checksum();
        if actual_checksum != expected_checksum {
            return Err(ReplayError::ChecksumMismatch {
                offset_ns: record.receive_offset_ns,
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }
        self.book = next;
        self.exchange_timestamp.clone_from(&data.timestamp);
        self.checksum_valid = true;
        self.synchronized = true;
        self.messages += 1;
        self.price_level_mutations += mutations;
        Ok(())
    }

    fn frame(&self) -> ReplayFrame {
        let spread = self
            .book
            .asks
            .first()
            .zip(self.book.bids.first())
            .map(|(ask, bid)| {
                format_scaled(ask.price_atoms - bid.price_atoms, self.book.price_precision)
            });
        ReplayFrame {
            elapsed_ns: self.elapsed_ns,
            exchange_timestamp: self.exchange_timestamp.clone(),
            bids: self.book.bids.iter().map(public_level).collect(),
            asks: self.book.asks.iter().map(public_level).collect(),
            spread,
            checksum_valid: self.checksum_valid,
            synchronized: self.synchronized,
            messages: self.messages,
            price_level_mutations: self.price_level_mutations,
            last_record_kind: self.last_record_kind,
            last_payload: self.last_payload.clone(),
        }
    }

    fn remember(&mut self, record: &TapeRecord) {
        self.last_record_kind = Some(record.kind);
        self.last_payload = Some(String::from_utf8_lossy(&record.payload).into_owned());
    }
}

#[derive(Deserialize)]
struct WireMessage {
    #[serde(rename = "type")]
    message_type: String,
    data: Vec<WireBook>,
}

#[derive(Deserialize)]
struct WireBook {
    symbol: String,
    #[serde(default)]
    bids: Vec<WireLevel>,
    #[serde(default)]
    asks: Vec<WireLevel>,
    checksum: u32,
    timestamp: Option<String>,
}

#[derive(Deserialize)]
struct WireLevel {
    price: Value,
    #[serde(rename = "qty")]
    quantity: Value,
}

fn public_level(level: &Level) -> BookLevel {
    BookLevel {
        price: level.price.to_string(),
        quantity: level.quantity.to_string(),
    }
}

fn invalid(record: &crate::TapeRecord, message: &str) -> ReplayError {
    ReplayError::InvalidMessage {
        offset_ns: record.receive_offset_ns,
        message: message.into(),
    }
}

fn decimal_text(value: &Value) -> Result<&str, String> {
    match value {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(value.as_str()),
        _ => Err("decimal value must be a number or string".into()),
    }
}

fn parse_scaled(value: &str, scale: u8) -> Result<i64, String> {
    if value.starts_with('-') || value.starts_with('+') || value.contains(['e', 'E']) {
        return Err(format!("unsupported decimal {value}"));
    }
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("invalid decimal {value}"));
    }
    let scale = scale as usize;
    if fraction.len() > scale && fraction[scale..].bytes().any(|byte| byte != b'0') {
        return Err(format!("decimal {value} exceeds precision {scale}"));
    }
    let fraction = &fraction[..fraction.len().min(scale)];
    let factor = 10_i64
        .checked_pow(scale as u32)
        .ok_or_else(|| "decimal precision is too large".to_string())?;
    let whole: i64 = whole
        .parse()
        .map_err(|_| format!("decimal {value} exceeds supported range"))?;
    let mut fraction_atoms: i64 = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse()
            .map_err(|_| format!("invalid decimal {value}"))?
    };
    for _ in fraction.len()..scale {
        fraction_atoms *= 10;
    }
    whole
        .checked_mul(factor)
        .and_then(|atoms| atoms.checked_add(fraction_atoms))
        .ok_or_else(|| format!("decimal {value} exceeds supported range"))
}

fn format_scaled(atoms: i64, scale: u8) -> String {
    if scale == 0 {
        return atoms.to_string();
    }
    let factor = 10_i64.pow(scale as u32);
    let sign = if atoms < 0 { "-" } else { "" };
    let absolute = atoms.unsigned_abs();
    format!(
        "{sign}{}.{:0width$}",
        absolute / factor as u64,
        absolute % factor as u64,
        width = scale as usize
    )
}
