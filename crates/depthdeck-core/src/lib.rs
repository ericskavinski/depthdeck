#![forbid(unsafe_code)]

mod replay;
mod tape;

pub use replay::{BookLevel, ReplayError, ReplayFrame, ReplaySession};
#[cfg(not(target_arch = "wasm32"))]
pub use tape::TapeWriter;
pub use tape::{RecordKind, TapeError, TapeMetadata, TapeReader, TapeRecord};

/// Computes Kraken's CRC32 checksum over the top ten asks and bids.
///
pub fn kraken_checksum(asks: &[(&str, &str)], bids: &[(&str, &str)]) -> u32 {
    let mut input = String::with_capacity(256);
    for (price, quantity) in asks.iter().take(10).chain(bids.iter().take(10)) {
        append_checksum_decimal(&mut input, price);
        append_checksum_decimal(&mut input, quantity);
    }
    crc32fast::hash(input.as_bytes())
}

fn append_checksum_decimal(output: &mut String, value: &str) {
    let digits = value.bytes().filter(|byte| *byte != b'.');
    let mut significant = false;
    for digit in digits {
        if digit != b'0' || significant {
            significant = true;
            output.push(char::from(digit));
        }
    }
    if !significant {
        output.push('0');
    }
}
