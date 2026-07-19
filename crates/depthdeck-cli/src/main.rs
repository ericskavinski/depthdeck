mod capture;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use depthdeck_core::{RecordKind, ReplaySession, TapeReader, generate_synthetic_tape};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    name = "depthdeck",
    version,
    about = "Deterministic L2 market capture and replay"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Capture a checksummed Kraken Spot v2 L2 book.
    Capture {
        #[arg(long, default_value = "BTC/USD")]
        symbol: String,
        #[arg(long, default_value_t = 100)]
        depth: u16,
        #[arg(long, value_parser = parse_duration)]
        duration: Option<Duration>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Print tape metadata and storage statistics.
    Inspect {
        tape: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Decode every record and verify every exchange checksum.
    Verify {
        tape: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Replay a tape and emit reconstructed frames as NDJSON.
    Replay {
        tape: PathBuf,
        #[arg(long, default_value = "1")]
        speed: String,
        #[arg(long, value_enum, default_value_t = Emit::Snapshots)]
        emit: Emit,
    },
    /// Export tape records as newline-delimited JSON.
    Export {
        tape: PathBuf,
        #[arg(long, value_enum, default_value_t = ExportFormat::Ndjson)]
        format: ExportFormat,
        #[arg(long, default_value = "-")]
        output: String,
    },
    /// Measure maximum-speed deterministic replay throughput.
    Bench {
        tape: Option<PathBuf>,
        #[arg(long, default_value_t = 20)]
        iterations: u32,
        #[arg(long)]
        json: bool,
    },
    #[command(hide = true)]
    GenerateDemo {
        output: PathBuf,
        #[arg(long, default_value_t = 90)]
        duration: u64,
        #[arg(long, default_value_t = 100)]
        rate: u32,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Emit {
    Updates,
    Snapshots,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportFormat {
    Ndjson,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Capture {
            symbol,
            depth,
            duration,
            output,
            force,
        } => {
            capture::run(capture::CaptureOptions {
                symbol,
                depth,
                duration,
                output,
                force,
            })
            .await
        }
        Command::Inspect { tape, json } => inspect(&tape, json),
        Command::Verify { tape, json } => verify(&tape, json),
        Command::Replay { tape, speed, emit } => replay(&tape, &speed, emit).await,
        Command::Export {
            tape,
            format: _,
            output,
        } => export(&tape, &output),
        Command::Bench {
            tape,
            iterations,
            json,
        } => bench(tape.as_deref(), iterations, json),
        Command::GenerateDemo {
            output,
            duration,
            rate,
        } => generate_demo(&output, duration, rate),
    }
}

fn read_tape(path: &Path) -> Result<(Vec<u8>, TapeReader)> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let reader =
        TapeReader::open(&bytes).with_context(|| format!("failed to open {}", path.display()))?;
    Ok((bytes, reader))
}

fn inspect(path: &Path, machine_readable: bool) -> Result<()> {
    let (bytes, reader) = read_tape(path)?;
    let duration_ns = reader.duration_ns();
    let summary = json!({
        "path": path,
        "bytes": bytes.len(),
        "metadata": reader.metadata(),
        "records": reader.records().len(),
        "chunks": reader.chunk_count(),
        "duration_ns": duration_ns,
        "compression_ratio": reader.compression_ratio(),
    });
    if machine_readable {
        println!("{}", serde_json::to_string(&summary)?);
    } else {
        println!("DepthDeck tape: {}", path.display());
        println!("  venue:   {}", reader.metadata().exchange);
        println!("  symbol:  {}", reader.metadata().symbol);
        println!("  depth:   {}", reader.metadata().depth);
        println!("  records: {}", reader.records().len());
        println!("  chunks:  {}", reader.chunk_count());
        println!("  duration: {:.3}s", duration_ns as f64 / 1e9);
        println!("  size:    {} bytes", bytes.len());
    }
    Ok(())
}

fn verify(path: &Path, machine_readable: bool) -> Result<()> {
    let (_, reader) = read_tape(path)?;
    let duration_ns = reader.duration_ns();
    let mut replay = ReplaySession::new(reader)?;
    let frame = replay.advance(duration_ns)?;
    if frame.messages == 0 {
        bail!("tape contains no valid book messages");
    }
    let summary = json!({
        "valid": true,
        "checksum_valid": true,
        "synchronized_at_end": frame.synchronized,
        "messages": frame.messages,
        "price_level_mutations": frame.price_level_mutations,
        "duration_ns": duration_ns,
        "digest": replay.digest(),
    });
    if machine_readable {
        println!("{}", serde_json::to_string(&summary)?);
    } else {
        println!("verified {}", path.display());
        println!("  messages:  {}", frame.messages);
        println!("  mutations: {}", frame.price_level_mutations);
        println!("  digest:    {}", replay.digest());
        println!("  all exchange checksums valid: true");
        println!("  synchronized at end: {}", frame.synchronized);
    }
    Ok(())
}

async fn replay(path: &Path, speed: &str, emit: Emit) -> Result<()> {
    let (_, reader) = read_tape(path)?;
    let records = reader.records().to_vec();
    let mut replay = ReplaySession::new(reader)?;
    let speed = if speed.eq_ignore_ascii_case("max") {
        None
    } else {
        let parsed: f64 = speed
            .parse()
            .context("speed must be a positive number or 'max'")?;
        if !parsed.is_finite() || parsed <= 0.0 {
            bail!("speed must be a positive number or 'max'");
        }
        Some(parsed)
    };
    let mut previous = 0_u64;
    for record in records {
        if let Some(multiplier) = speed {
            let wait_ns =
                (record.receive_offset_ns.saturating_sub(previous) as f64 / multiplier) as u64;
            tokio::time::sleep(Duration::from_nanos(wait_ns)).await;
        }
        previous = record.receive_offset_ns;
        let frame = replay.advance(record.receive_offset_ns)?;
        let selected = matches!(
            (emit, record.kind),
            (Emit::Snapshots, RecordKind::Snapshot) | (Emit::Updates, RecordKind::Update)
        );
        if selected {
            println!("{}", serde_json::to_string(&frame)?);
        }
    }
    Ok(())
}

fn export(path: &Path, output: &str) -> Result<()> {
    let (_, reader) = read_tape(path)?;
    let mut destination: Box<dyn Write> = if output == "-" {
        Box::new(std::io::stdout().lock())
    } else {
        Box::new(fs::File::create(output).with_context(|| format!("failed to create {output}"))?)
    };
    for record in reader.records() {
        let payload = serde_json::from_slice::<serde_json::Value>(&record.payload)
            .unwrap_or_else(|_| json!(String::from_utf8_lossy(&record.payload)));
        serde_json::to_writer(
            &mut destination,
            &json!({
                "receive_offset_ns": record.receive_offset_ns,
                "kind": record.kind,
                "payload": payload,
            }),
        )?;
        destination.write_all(b"\n")?;
    }
    Ok(())
}

fn bench(path: Option<&Path>, iterations: u32, machine_readable: bool) -> Result<()> {
    if iterations == 0 {
        bail!("iterations must be positive");
    }
    let bytes = match path {
        Some(path) => {
            fs::read(path).with_context(|| format!("failed to read {}", path.display()))?
        }
        None => generate_synthetic_tape(10, 1_000)?,
    };
    let reader = TapeReader::open(&bytes)?;
    let duration_ns = reader.duration_ns();
    let started = Instant::now();
    let mut messages = 0_u64;
    let mut mutations = 0_u64;
    let mut digest = String::new();
    for _ in 0..iterations {
        let mut replay = ReplaySession::new(reader.clone())?;
        let frame = replay.advance(duration_ns)?;
        messages = messages.saturating_add(frame.messages);
        mutations = mutations.saturating_add(frame.price_level_mutations);
        digest = replay.digest();
    }
    let wall_seconds = started.elapsed().as_secs_f64();
    let messages_per_second = messages as f64 / wall_seconds;
    let mutations_per_second = mutations as f64 / wall_seconds;
    let mut seek_replay = ReplaySession::new(reader)?;
    let seek_started = Instant::now();
    seek_replay.seek(duration_ns / 2)?;
    let midpoint_seek_ms = seek_started.elapsed().as_secs_f64() * 1_000.0;
    let summary = json!({
        "iterations": iterations,
        "wall_seconds": wall_seconds,
        "messages": messages,
        "messages_per_second": messages_per_second,
        "price_level_mutations": mutations,
        "mutations_per_second": mutations_per_second,
        "midpoint_seek_ms": midpoint_seek_ms,
        "digest": digest,
    });
    if machine_readable {
        println!("{}", serde_json::to_string(&summary)?);
    } else {
        println!("DepthDeck replay benchmark");
        println!("  iterations: {iterations}");
        println!("  messages:   {messages}");
        println!("  mutations:  {mutations}");
        println!("  throughput: {:.0} messages/s", messages_per_second);
        println!("              {:.0} mutations/s", mutations_per_second);
        println!("  midpoint seek: {midpoint_seek_ms:.2}ms");
        println!("  digest:     {digest}");
    }
    Ok(())
}

fn generate_demo(path: &Path, duration: u64, rate: u32) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite {}", path.display());
    }
    let bytes = generate_synthetic_tape(duration, rate)?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    humantime::parse_duration(value).map_err(|error| error.to_string())
}
