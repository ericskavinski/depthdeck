use std::{fs, io::Cursor};

use assert_cmd::Command;
use depthdeck_core::{RecordKind, TapeMetadata, TapeRecord, TapeWriter, kraken_checksum};

#[test]
fn captured_tape_can_be_inspected_and_verified() {
    let directory = tempfile::tempdir().unwrap();
    let tape = directory.path().join("capture.ddt");
    write_valid_tape(&tape);

    Command::cargo_bin("depthdeck")
        .unwrap()
        .args(["inspect", tape.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("BTC/USD"));

    Command::cargo_bin("depthdeck")
        .unwrap()
        .args(["verify", tape.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"checksum_valid\":true"));
}

fn write_valid_tape(path: &std::path::Path) {
    let metadata = TapeMetadata {
        exchange: "kraken-spot-v2".into(),
        symbol: "BTC/USD".into(),
        depth: 10,
        price_precision: 1,
        quantity_precision: 8,
        capture_started_unix_ns: 1_700_000_000_000_000_000,
        generator: "depthdeck-cli-test".into(),
    };
    let checksum = kraken_checksum(&[("101.0", "2.00000000")], &[("100.0", "1.50000000")]);
    let payload = format!(
        r#"{{"channel":"book","type":"snapshot","data":[{{"symbol":"BTC/USD","bids":[{{"price":"100.0","qty":"1.50000000"}}],"asks":[{{"price":"101.0","qty":"2.00000000"}}],"checksum":{checksum},"timestamp":"2026-07-19T12:00:00Z"}}]}}"#
    );
    let mut writer = TapeWriter::new(Cursor::new(Vec::new()), metadata).unwrap();
    writer
        .push(TapeRecord {
            receive_offset_ns: 1,
            kind: RecordKind::Snapshot,
            payload: payload.into_bytes(),
        })
        .unwrap();
    fs::write(path, writer.finish().unwrap().into_inner()).unwrap();
}
