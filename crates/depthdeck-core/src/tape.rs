#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::io::{Cursor, Read, Seek};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAGIC: &[u8; 8] = b"DPTHDCK1";
const VERSION: u16 = 1;
const CHUNK_MARKER: &[u8; 4] = b"CHNK";
const INDEX_MARKER: &[u8; 4] = b"INDX";
const END_MARKER: &[u8; 4] = b"END!";
#[cfg(not(target_arch = "wasm32"))]
const CHUNK_TIME_NS: u64 = 1_000_000_000;
#[cfg(not(target_arch = "wasm32"))]
const CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapeMetadata {
    pub exchange: String,
    pub symbol: String,
    pub depth: u16,
    pub price_precision: u8,
    pub quantity_precision: u8,
    pub capture_started_unix_ns: i64,
    pub generator: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordKind {
    ConnectionOpened,
    ConnectionLost,
    Snapshot,
    Update,
    ChecksumMismatch,
}

impl RecordKind {
    #[cfg(not(target_arch = "wasm32"))]
    fn code(self) -> u8 {
        match self {
            Self::ConnectionOpened => 1,
            Self::ConnectionLost => 2,
            Self::Snapshot => 3,
            Self::Update => 4,
            Self::ChecksumMismatch => 5,
        }
    }

    fn from_code(code: u8) -> Result<Self, TapeError> {
        match code {
            1 => Ok(Self::ConnectionOpened),
            2 => Ok(Self::ConnectionLost),
            3 => Ok(Self::Snapshot),
            4 => Ok(Self::Update),
            5 => Ok(Self::ChecksumMismatch),
            _ => Err(TapeError::Invalid(format!("unknown record kind {code}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TapeRecord {
    pub receive_offset_ns: u64,
    pub kind: RecordKind,
    pub payload: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum TapeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid tape: {0}")]
    Invalid(String),
    #[error("unsupported tape version {0}")]
    UnsupportedVersion(u16),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ChunkIndex {
    first_offset_ns: u64,
    last_offset_ns: u64,
    file_offset: u64,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct TapeWriter<W> {
    output: W,
    metadata: TapeMetadata,
    chunk: Vec<u8>,
    chunk_records: u32,
    chunk_first_ns: Option<u64>,
    chunk_last_ns: u64,
    last_record_ns: Option<u64>,
    index: Vec<ChunkIndex>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<W: Write + Seek> TapeWriter<W> {
    pub fn new(mut output: W, metadata: TapeMetadata) -> Result<Self, TapeError> {
        let encoded_metadata = serde_json::to_vec(&metadata)?;
        output.write_all(MAGIC)?;
        write_u16(&mut output, VERSION)?;
        write_u32(&mut output, encoded_metadata.len() as u32)?;
        output.write_all(&encoded_metadata)?;
        Ok(Self {
            output,
            metadata,
            chunk: Vec::new(),
            chunk_records: 0,
            chunk_first_ns: None,
            chunk_last_ns: 0,
            last_record_ns: None,
            index: Vec::new(),
        })
    }

    pub fn metadata(&self) -> &TapeMetadata {
        &self.metadata
    }

    pub fn push(&mut self, record: TapeRecord) -> Result<(), TapeError> {
        if let Some(previous) = self.last_record_ns
            && record.receive_offset_ns < previous
        {
            return Err(TapeError::Invalid(
                "record receive offsets must be monotonic".into(),
            ));
        }
        let encoded_len = 8 + 1 + 4 + record.payload.len();
        if let Some(first) = self.chunk_first_ns
            && (record.receive_offset_ns.saturating_sub(first) >= CHUNK_TIME_NS
                || self.chunk.len() + encoded_len > CHUNK_SIZE)
        {
            self.flush_chunk()?;
        }
        self.chunk_first_ns.get_or_insert(record.receive_offset_ns);
        self.chunk_last_ns = record.receive_offset_ns;
        self.last_record_ns = Some(record.receive_offset_ns);
        write_u64(&mut self.chunk, record.receive_offset_ns)?;
        self.chunk.push(record.kind.code());
        write_u32(&mut self.chunk, record.payload.len() as u32)?;
        self.chunk.extend_from_slice(&record.payload);
        self.chunk_records += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<W, TapeError> {
        self.flush_chunk()?;
        self.output.write_all(INDEX_MARKER)?;
        let encoded_index = serde_json::to_vec(&self.index)?;
        write_u32(&mut self.output, encoded_index.len() as u32)?;
        self.output.write_all(&encoded_index)?;
        self.output.write_all(END_MARKER)?;
        self.output.flush()?;
        Ok(self.output)
    }

    fn flush_chunk(&mut self) -> Result<(), TapeError> {
        let Some(first_offset_ns) = self.chunk_first_ns else {
            return Ok(());
        };
        let file_offset = self.output.stream_position()?;
        let compressed = zstd::stream::encode_all(Cursor::new(&self.chunk), 3)?;
        self.output.write_all(CHUNK_MARKER)?;
        write_u64(&mut self.output, first_offset_ns)?;
        write_u64(&mut self.output, self.chunk_last_ns)?;
        write_u32(&mut self.output, self.chunk_records)?;
        write_u32(&mut self.output, self.chunk.len() as u32)?;
        write_u32(&mut self.output, compressed.len() as u32)?;
        write_u32(&mut self.output, crc32fast::hash(&self.chunk))?;
        self.output.write_all(&compressed)?;
        self.index.push(ChunkIndex {
            first_offset_ns,
            last_offset_ns: self.chunk_last_ns,
            file_offset,
        });
        self.chunk.clear();
        self.chunk_records = 0;
        self.chunk_first_ns = None;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TapeReader {
    metadata: TapeMetadata,
    records: Vec<TapeRecord>,
    chunk_count: usize,
    compressed_bytes: usize,
    uncompressed_bytes: usize,
}

impl TapeReader {
    pub fn open(bytes: &[u8]) -> Result<Self, TapeError> {
        let mut cursor = Cursor::new(bytes);
        let mut magic = [0_u8; 8];
        read_exact_at(&mut cursor, &mut magic)?;
        if &magic != MAGIC {
            return Err(TapeError::Invalid("bad magic at byte 0".into()));
        }
        let version = read_u16(&mut cursor)?;
        if version != VERSION {
            return Err(TapeError::UnsupportedVersion(version));
        }
        let metadata_len = read_u32(&mut cursor)? as usize;
        let mut metadata_bytes = vec![0_u8; metadata_len];
        read_exact_at(&mut cursor, &mut metadata_bytes)?;
        let metadata = serde_json::from_slice(&metadata_bytes)?;
        let mut records: Vec<TapeRecord> = Vec::new();
        let mut chunk_count = 0;
        let mut compressed_bytes = 0;
        let mut uncompressed_bytes = 0;
        let mut observed_index = Vec::new();

        loop {
            let marker_offset = cursor.position();
            let mut marker = [0_u8; 4];
            read_exact_at(&mut cursor, &mut marker)?;
            if &marker == CHUNK_MARKER {
                let first = read_u64(&mut cursor)?;
                let last = read_u64(&mut cursor)?;
                let record_count = read_u32(&mut cursor)?;
                let expected_len = read_u32(&mut cursor)? as usize;
                let compressed_len = read_u32(&mut cursor)? as usize;
                let expected_crc = read_u32(&mut cursor)?;
                let mut compressed = vec![0_u8; compressed_len];
                read_exact_at(&mut cursor, &mut compressed)?;
                let decoded = decompress(compressed)?;
                if decoded.len() != expected_len || crc32fast::hash(&decoded) != expected_crc {
                    return Err(TapeError::Invalid(format!(
                        "chunk integrity check failed at byte {marker_offset}"
                    )));
                }
                let parsed = decode_records(&decoded, record_count)?;
                if parsed
                    .windows(2)
                    .any(|pair| pair[1].receive_offset_ns < pair[0].receive_offset_ns)
                    || records
                        .last()
                        .zip(parsed.first())
                        .is_some_and(|(previous, next)| {
                            next.receive_offset_ns < previous.receive_offset_ns
                        })
                {
                    return Err(TapeError::Invalid(format!(
                        "non-monotonic record offset in chunk at byte {marker_offset}"
                    )));
                }
                if parsed.first().map(|item| item.receive_offset_ns) != Some(first)
                    || parsed.last().map(|item| item.receive_offset_ns) != Some(last)
                {
                    return Err(TapeError::Invalid(format!(
                        "chunk time range mismatch at byte {marker_offset}"
                    )));
                }
                records.extend(parsed);
                chunk_count += 1;
                compressed_bytes += compressed_len;
                uncompressed_bytes += expected_len;
                observed_index.push(ChunkIndex {
                    first_offset_ns: first,
                    last_offset_ns: last,
                    file_offset: marker_offset,
                });
            } else if &marker == INDEX_MARKER {
                let index_len = read_u32(&mut cursor)? as usize;
                let mut encoded_index = vec![0_u8; index_len];
                read_exact_at(&mut cursor, &mut encoded_index)?;
                let index: Vec<ChunkIndex> = serde_json::from_slice(&encoded_index)?;
                if index != observed_index {
                    return Err(TapeError::Invalid("footer chunk index mismatch".into()));
                }
                let mut end = [0_u8; 4];
                read_exact_at(&mut cursor, &mut end)?;
                if &end != END_MARKER {
                    return Err(TapeError::Invalid(format!(
                        "missing end marker at byte {}",
                        cursor.position().saturating_sub(4)
                    )));
                }
                if cursor.position() != bytes.len() as u64 {
                    return Err(TapeError::Invalid("trailing bytes after footer".into()));
                }
                break;
            } else {
                return Err(TapeError::Invalid(format!(
                    "unknown marker at byte {marker_offset}"
                )));
            }
        }

        Ok(Self {
            metadata,
            records,
            chunk_count,
            compressed_bytes,
            uncompressed_bytes,
        })
    }

    pub fn metadata(&self) -> &TapeMetadata {
        &self.metadata
    }

    pub fn records(&self) -> &[TapeRecord] {
        &self.records
    }

    pub fn duration_ns(&self) -> u64 {
        self.records
            .last()
            .map_or(0, |record| record.receive_offset_ns)
    }

    pub fn chunk_count(&self) -> usize {
        self.chunk_count
    }

    pub fn compression_ratio(&self) -> f64 {
        if self.uncompressed_bytes == 0 {
            return 1.0;
        }
        self.compressed_bytes as f64 / self.uncompressed_bytes as f64
    }
}

fn decode_records(bytes: &[u8], count: u32) -> Result<Vec<TapeRecord>, TapeError> {
    let mut cursor = Cursor::new(bytes);
    let mut records = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let receive_offset_ns = read_u64(&mut cursor)?;
        let mut kind = [0_u8; 1];
        read_exact_at(&mut cursor, &mut kind)?;
        let payload_len = read_u32(&mut cursor)? as usize;
        let mut payload = vec![0_u8; payload_len];
        read_exact_at(&mut cursor, &mut payload)?;
        records.push(TapeRecord {
            receive_offset_ns,
            kind: RecordKind::from_code(kind[0])?,
            payload,
        });
    }
    if cursor.position() != bytes.len() as u64 {
        return Err(TapeError::Invalid("extra bytes in chunk".into()));
    }
    Ok(records)
}

fn read_exact_at<R: Read + Seek>(reader: &mut R, bytes: &mut [u8]) -> Result<(), TapeError> {
    let offset = reader.stream_position()?;
    reader
        .read_exact(bytes)
        .map_err(|error| TapeError::Invalid(format!("truncated tape at byte {offset}: {error}")))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_u16<W: Write>(writer: &mut W, value: u16) -> Result<(), std::io::Error> {
    writer.write_all(&value.to_le_bytes())
}

#[cfg(not(target_arch = "wasm32"))]
fn write_u32<W: Write>(writer: &mut W, value: u32) -> Result<(), std::io::Error> {
    writer.write_all(&value.to_le_bytes())
}

#[cfg(not(target_arch = "wasm32"))]
fn write_u64<W: Write>(writer: &mut W, value: u64) -> Result<(), std::io::Error> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u16<R: Read + Seek>(reader: &mut R) -> Result<u16, TapeError> {
    let mut bytes = [0_u8; 2];
    read_exact_at(reader, &mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32<R: Read + Seek>(reader: &mut R) -> Result<u32, TapeError> {
    let mut bytes = [0_u8; 4];
    read_exact_at(reader, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read + Seek>(reader: &mut R) -> Result<u64, TapeError> {
    let mut bytes = [0_u8; 8];
    read_exact_at(reader, &mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(not(target_arch = "wasm32"))]
fn decompress(bytes: Vec<u8>) -> Result<Vec<u8>, TapeError> {
    Ok(zstd::stream::decode_all(Cursor::new(bytes))?)
}

#[cfg(target_arch = "wasm32")]
fn decompress(bytes: Vec<u8>) -> Result<Vec<u8>, TapeError> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(Cursor::new(bytes))
        .map_err(|error| TapeError::Invalid(format!("invalid zstd stream: {error}")))?;
    let mut decoded = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .map_err(|error| TapeError::Invalid(format!("invalid zstd stream: {error}")))?;
    Ok(decoded)
}
