use std::io::Cursor;

use serde_json::json;

use crate::{RecordKind, TapeError, TapeMetadata, TapeRecord, TapeWriter, kraken_checksum};

pub fn generate_synthetic_tape(
    duration_seconds: u64,
    updates_per_second: u32,
) -> Result<Vec<u8>, TapeError> {
    if duration_seconds == 0 || updates_per_second == 0 {
        return Err(TapeError::Invalid(
            "synthetic duration and update rate must be positive".into(),
        ));
    }
    let metadata = TapeMetadata {
        exchange: "synthetic-kraken-spot-v2".into(),
        symbol: "BTC/USD".into(),
        depth: 100,
        price_precision: 1,
        quantity_precision: 8,
        capture_started_unix_ns: 1_753_012_800_000_000_000,
        generator: "depthdeck-synthetic-v1".into(),
    };
    let mut writer = TapeWriter::new(Cursor::new(Vec::new()), metadata)?;
    writer.push(TapeRecord {
        receive_offset_ns: 0,
        kind: RecordKind::ConnectionOpened,
        payload: br#"{"source":"synthetic","seed":20260719}"#.to_vec(),
    })?;

    let mut bids = levels(45_000, -1, 20);
    let mut asks = levels(45_010, 1, 20);
    let snapshot_checksum = checksum(&asks, &bids);
    writer.push(TapeRecord {
        receive_offset_ns: 1,
        kind: RecordKind::Snapshot,
        payload: book_payload(
            "snapshot",
            &bids,
            &asks,
            snapshot_checksum,
            synthetic_timestamp(0),
        ),
    })?;

    let update_count = duration_seconds.saturating_mul(updates_per_second as u64);
    for index in 1..=update_count {
        let side_is_bid = index % 2 == 0;
        let level_index = ((index * 17) % 20) as usize;
        let quantity = format!("{}.{:08}", 1 + (index % 4), (index * 79_139) % 100_000_000);
        let (updates, checksum) = if side_is_bid {
            bids[level_index].1.clone_from(&quantity);
            (
                (vec![bids[level_index].clone()], Vec::new()),
                checksum(&asks, &bids),
            )
        } else {
            asks[level_index].1.clone_from(&quantity);
            (
                (Vec::new(), vec![asks[level_index].clone()]),
                checksum(&asks, &bids),
            )
        };
        let offset_ns = index.saturating_mul(1_000_000_000) / updates_per_second as u64;
        writer.push(TapeRecord {
            receive_offset_ns: offset_ns,
            kind: RecordKind::Update,
            payload: book_payload(
                "update",
                &updates.0,
                &updates.1,
                checksum,
                synthetic_timestamp(offset_ns),
            ),
        })?;
    }
    Ok(writer.finish()?.into_inner())
}

fn levels(start_tenths: i64, direction: i64, count: usize) -> Vec<(String, String)> {
    (0..count)
        .map(|index| {
            let price = start_tenths + direction * index as i64;
            (
                format!("{}.{:01}", price / 10, price.unsigned_abs() % 10),
                format!("{}.{:08}", 1 + index % 5, index * 3_791 % 100_000_000),
            )
        })
        .collect()
}

fn checksum(asks: &[(String, String)], bids: &[(String, String)]) -> u32 {
    let asks: Vec<_> = asks
        .iter()
        .map(|(price, quantity)| (price.as_str(), quantity.as_str()))
        .collect();
    let bids: Vec<_> = bids
        .iter()
        .map(|(price, quantity)| (price.as_str(), quantity.as_str()))
        .collect();
    kraken_checksum(&asks, &bids)
}

fn book_payload(
    message_type: &str,
    bids: &[(String, String)],
    asks: &[(String, String)],
    checksum: u32,
    timestamp: String,
) -> Vec<u8> {
    let levels = |items: &[(String, String)]| {
        items
            .iter()
            .map(|(price, quantity)| json!({ "price": price, "qty": quantity }))
            .collect::<Vec<_>>()
    };
    serde_json::to_vec(&json!({
        "channel": "book",
        "type": message_type,
        "data": [{
            "symbol": "BTC/USD",
            "bids": levels(bids),
            "asks": levels(asks),
            "checksum": checksum,
            "timestamp": timestamp,
        }]
    }))
    .expect("synthetic messages contain only serializable values")
}

fn synthetic_timestamp(offset_ns: u64) -> String {
    let total_seconds = offset_ns / 1_000_000_000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let micros = (offset_ns % 1_000_000_000) / 1_000;
    format!("2026-07-19T12:{minutes:02}:{seconds:02}.{micros:06}Z")
}
