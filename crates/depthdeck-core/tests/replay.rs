use std::io::Cursor;

use depthdeck_core::{
    RecordKind, ReplaySession, TapeMetadata, TapeReader, TapeRecord, TapeWriter, kraken_checksum,
};

fn tape_with(records: Vec<TapeRecord>) -> TapeReader {
    let metadata = TapeMetadata {
        exchange: "kraken-spot-v2".into(),
        symbol: "BTC/USD".into(),
        depth: 10,
        price_precision: 1,
        quantity_precision: 8,
        capture_started_unix_ns: 1_700_000_000_000_000_000,
        generator: "depthdeck-test".into(),
    };
    let mut writer = TapeWriter::new(Cursor::new(Vec::new()), metadata).unwrap();
    for record in records {
        writer.push(record).unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    TapeReader::open(&bytes).unwrap()
}

#[test]
fn replay_applies_a_valid_snapshot_through_the_public_session() {
    let asks = [("101.0", "2.00000000")];
    let bids = [("100.0", "1.50000000")];
    let checksum = kraken_checksum(&asks, &bids);
    let payload = format!(
        r#"{{"channel":"book","type":"snapshot","data":[{{"symbol":"BTC/USD","bids":[{{"price":"100.0","qty":"1.50000000"}}],"asks":[{{"price":"101.0","qty":"2.00000000"}}],"checksum":{checksum},"timestamp":"2026-07-19T12:00:00.000000Z"}}]}}"#
    );
    let reader = tape_with(vec![TapeRecord {
        receive_offset_ns: 10,
        kind: RecordKind::Snapshot,
        payload: payload.into_bytes(),
    }]);

    let mut replay = ReplaySession::new(reader).unwrap();
    let frame = replay.advance(10).unwrap();

    assert!(frame.synchronized);
    assert!(frame.checksum_valid);
    assert_eq!(frame.bids[0].price, "100.0");
    assert_eq!(frame.asks[0].quantity, "2.00000000");
    assert_eq!(frame.spread.as_deref(), Some("1.0"));
    assert_eq!(frame.messages, 1);
}

#[test]
fn checksum_failure_does_not_publish_the_invalid_update() {
    let snapshot_checksum = kraken_checksum(&[("101.0", "2.00000000")], &[("100.0", "1.50000000")]);
    let snapshot = format!(
        r#"{{"channel":"book","type":"snapshot","data":[{{"symbol":"BTC/USD","bids":[{{"price":"100.0","qty":"1.50000000"}}],"asks":[{{"price":"101.0","qty":"2.00000000"}}],"checksum":{snapshot_checksum},"timestamp":"2026-07-19T12:00:00Z"}}]}}"#
    );
    let invalid_update = r#"{"channel":"book","type":"update","data":[{"symbol":"BTC/USD","bids":[{"price":"100.0","qty":"9.00000000"}],"asks":[],"checksum":1,"timestamp":"2026-07-19T12:00:01Z"}]}"#;
    let reader = tape_with(vec![
        TapeRecord {
            receive_offset_ns: 10,
            kind: RecordKind::Snapshot,
            payload: snapshot.into_bytes(),
        },
        TapeRecord {
            receive_offset_ns: 20,
            kind: RecordKind::Update,
            payload: invalid_update.as_bytes().to_vec(),
        },
    ]);

    let mut replay = ReplaySession::new(reader).unwrap();
    replay.advance(10).unwrap();
    let valid_digest = replay.digest();
    let error = replay.advance(20).unwrap_err().to_string();

    assert!(error.contains("checksum mismatch"), "{error}");
    assert_eq!(replay.digest(), valid_digest);
    let retry = replay.advance(20).unwrap_err().to_string();
    assert_eq!(retry, error, "the rejected record must not be skipped");
}
