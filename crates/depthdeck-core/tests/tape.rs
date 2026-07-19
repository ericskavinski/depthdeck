use std::io::Cursor;

use depthdeck_core::{RecordKind, TapeMetadata, TapeReader, TapeRecord, TapeWriter};

fn metadata() -> TapeMetadata {
    TapeMetadata {
        exchange: "kraken-spot-v2".into(),
        symbol: "BTC/USD".into(),
        depth: 100,
        price_precision: 1,
        quantity_precision: 8,
        capture_started_unix_ns: 1_700_000_000_000_000_000,
        generator: "depthdeck-test".into(),
    }
}

#[test]
fn tape_round_trip_preserves_metadata_and_records() {
    let mut writer = TapeWriter::new(Cursor::new(Vec::new()), metadata()).unwrap();
    writer
        .push(TapeRecord {
            receive_offset_ns: 42,
            kind: RecordKind::Snapshot,
            payload: br#"{"type":"snapshot"}"#.to_vec(),
        })
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();

    let reader = TapeReader::open(&bytes).unwrap();
    assert_eq!(reader.metadata(), &metadata());
    assert_eq!(
        reader.records(),
        &[TapeRecord {
            receive_offset_ns: 42,
            kind: RecordKind::Snapshot,
            payload: br#"{"type":"snapshot"}"#.to_vec(),
        }]
    );
    assert_eq!(reader.duration_ns(), 42);
}

#[test]
fn corrupted_footer_index_is_rejected() {
    let mut writer = TapeWriter::new(Cursor::new(Vec::new()), metadata()).unwrap();
    writer
        .push(TapeRecord {
            receive_offset_ns: 42,
            kind: RecordKind::Snapshot,
            payload: br#"{"type":"snapshot"}"#.to_vec(),
        })
        .unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    let footer = bytes
        .windows(4)
        .position(|window| window == b"INDX")
        .unwrap();
    let key = b"\"file_offset\":";
    let relative = bytes[footer..]
        .windows(key.len())
        .position(|window| window == key)
        .unwrap();
    let value = footer + relative + key.len();
    bytes[value] = if bytes[value] == b'9' { b'8' } else { b'9' };

    let error = TapeReader::open(&bytes).unwrap_err().to_string();
    assert!(error.contains("footer chunk index mismatch"), "{error}");
}

#[test]
fn corrupted_chunk_reports_its_byte_offset() {
    let mut writer = TapeWriter::new(Cursor::new(Vec::new()), metadata()).unwrap();
    writer
        .push(TapeRecord {
            receive_offset_ns: 42,
            kind: RecordKind::Snapshot,
            payload: br#"{"type":"snapshot"}"#.to_vec(),
        })
        .unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    let chunk = bytes
        .windows(4)
        .position(|window| window == b"CHNK")
        .unwrap();
    bytes[chunk + 32] ^= 0xff;

    let error = TapeReader::open(&bytes).unwrap_err().to_string();
    assert!(error.contains(&format!("byte {chunk}")), "{error}");
}
