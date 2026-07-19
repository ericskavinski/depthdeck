use depthdeck_core::{ReplaySession, TapeReader, generate_synthetic_tape};

#[test]
fn synthetic_tape_is_deterministic_and_fully_replayable() {
    let first = generate_synthetic_tape(2, 20).unwrap();
    let second = generate_synthetic_tape(2, 20).unwrap();
    assert_eq!(first, second);

    let reader = TapeReader::open(&first).unwrap();
    let mut replay = ReplaySession::new(reader).unwrap();
    let frame = replay.advance(2_000_000_000).unwrap();
    assert!(frame.synchronized);
    assert!(frame.checksum_valid);
    assert_eq!(frame.messages, 41);
}
