use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use depthdeck_core::{RecordKind, ReplaySession, TapeMetadata, TapeRecord, TapeWriter};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const KRAKEN_WEBSOCKET: &str = "wss://ws.kraken.com/v2";

pub struct CaptureOptions {
    pub symbol: String,
    pub depth: u16,
    pub duration: Option<Duration>,
    pub output: PathBuf,
    pub force: bool,
}

pub async fn run(options: CaptureOptions) -> Result<()> {
    if !matches!(options.depth, 10 | 25 | 100 | 500 | 1000) {
        bail!("depth must be one of 10, 25, 100, 500, or 1000");
    }
    if let Some(duration) = options.duration
        && duration.is_zero()
    {
        bail!("duration must be positive");
    }
    let part_path = PathBuf::from(format!("{}.part", options.output.display()));
    prepare_output(&options.output, &part_path, options.force)?;

    eprintln!("discovering {} instrument precision", options.symbol);
    let (price_precision, quantity_precision) = fetch_precision(&options.symbol).await?;
    let capture_started_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_nanos()
        .try_into()
        .context("system time exceeds DepthDeck's timestamp range")?;
    let metadata = TapeMetadata {
        exchange: "kraken-spot-v2".into(),
        symbol: options.symbol.clone(),
        depth: options.depth,
        price_precision,
        quantity_precision,
        capture_started_unix_ns,
        generator: format!("depthdeck/{}", env!("CARGO_PKG_VERSION")),
    };
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(true)
        .open(&part_path)
        .with_context(|| format!("failed to create {}", part_path.display()))?;
    let mut writer = TapeWriter::new(file, metadata.clone())?;
    let mut validator = ReplaySession::live(metadata)?;
    let started = Instant::now();
    let deadline = options.duration.map(|duration| started + duration);
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(true);
    });
    let mut backoff = Duration::from_secs(1);
    let mut saw_snapshot = false;

    loop {
        if should_stop(deadline, &shutdown_rx) {
            break;
        }
        eprintln!(
            "connecting to Kraken {} depth {}",
            options.symbol, options.depth
        );
        let connection =
            tokio::time::timeout(Duration::from_secs(15), connect_async(KRAKEN_WEBSOCKET)).await;
        let (mut socket, _) = match connection {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => {
                eprintln!(
                    "connection failed: {error}; retrying in {}",
                    humantime::format_duration(backoff)
                );
                wait_or_stop(backoff, deadline, &mut shutdown_rx).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
            Err(_) => {
                eprintln!(
                    "connection timed out; retrying in {}",
                    humantime::format_duration(backoff)
                );
                wait_or_stop(backoff, deadline, &mut shutdown_rx).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
        };
        backoff = Duration::from_secs(1);
        write_marker(
            &mut writer,
            &mut validator,
            started,
            RecordKind::ConnectionOpened,
            json!({"endpoint": KRAKEN_WEBSOCKET}),
        )?;
        let subscription = json!({
            "method": "subscribe",
            "params": {
                "channel": "book",
                "symbol": [options.symbol],
                "depth": options.depth,
                "snapshot": true,
            },
            "req_id": 1,
        });
        if let Err(error) = socket
            .send(Message::Text(subscription.to_string().into()))
            .await
        {
            write_marker(
                &mut writer,
                &mut validator,
                started,
                RecordKind::ConnectionLost,
                json!({"reason": error.to_string()}),
            )?;
            eprintln!(
                "subscription failed: {error}; retrying in {}",
                humantime::format_duration(backoff)
            );
            wait_or_stop(backoff, deadline, &mut shutdown_rx).await;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            continue;
        }
        let mut disconnect_reason = "stream closed".to_string();

        loop {
            let deadline_sleep = tokio::time::sleep_until(
                deadline
                    .map(tokio::time::Instant::from_std)
                    .unwrap_or_else(|| {
                        tokio::time::Instant::now() + Duration::from_secs(31_536_000)
                    }),
            );
            tokio::pin!(deadline_sleep);
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    disconnect_reason = "capture interrupted".into();
                    break;
                }
                _ = &mut deadline_sleep => {
                    disconnect_reason = "capture duration elapsed".into();
                    break;
                }
                message = socket.next() => {
                    let Some(message) = message else { break; };
                    match message {
                        Ok(Message::Text(text)) => {
                            let payload = text.as_bytes();
                            let Ok(value) = serde_json::from_slice::<Value>(payload) else { continue; };
                            if value.get("channel").and_then(Value::as_str) != Some("book") {
                                continue;
                            }
                            let kind = match value.get("type").and_then(Value::as_str) {
                                Some("snapshot") => RecordKind::Snapshot,
                                Some("update") => RecordKind::Update,
                                _ => continue,
                            };
                            let record = TapeRecord {
                                receive_offset_ns: started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX),
                                kind,
                                payload: payload.to_vec(),
                            };
                            match validator.apply_record(&record) {
                                Ok(_) => {
                                    saw_snapshot |= kind == RecordKind::Snapshot;
                                    writer.push(record)?;
                                }
                                Err(error) => {
                                    eprintln!("{error}; reconnecting for a fresh snapshot");
                                    write_marker(
                                        &mut writer,
                                        &mut validator,
                                        started,
                                        RecordKind::ChecksumMismatch,
                                        json!({"error": error.to_string(), "raw": value}),
                                    )?;
                                    disconnect_reason = error.to_string();
                                    break;
                                }
                            }
                        }
                        Ok(Message::Close(frame)) => {
                            disconnect_reason = frame.map_or_else(|| "server closed connection".into(), |frame| frame.reason.to_string());
                            break;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            disconnect_reason = error.to_string();
                            break;
                        }
                    }
                }
            }
        }
        write_marker(
            &mut writer,
            &mut validator,
            started,
            RecordKind::ConnectionLost,
            json!({"reason": disconnect_reason}),
        )?;
        if should_stop(deadline, &shutdown_rx) {
            break;
        }
        wait_or_stop(backoff, deadline, &mut shutdown_rx).await;
    }

    if !saw_snapshot {
        bail!(
            "capture ended before a valid snapshot arrived; partial tape remains at {}",
            part_path.display()
        );
    }
    let file = writer.finish()?;
    file.sync_all()?;
    drop(file);
    finalize_output(&part_path, &options.output, options.force).with_context(|| {
        format!(
            "failed to finalize {} as {}",
            part_path.display(),
            options.output.display()
        )
    })?;
    sync_output_directory(&options.output)?;
    eprintln!("wrote {}", options.output.display());
    Ok(())
}

#[cfg(not(windows))]
fn finalize_output(part: &Path, output: &Path, force: bool) -> std::io::Result<()> {
    if force {
        return fs::rename(part, output);
    }
    fs::hard_link(part, output)?;
    fs::remove_file(part)
}

#[cfg(windows)]
fn finalize_output(part: &Path, output: &Path, force: bool) -> std::io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    if !force {
        fs::hard_link(part, output)?;
        return fs::remove_file(part);
    }
    if !output.exists() {
        return fs::rename(part, output);
    }
    let replaced: Vec<u16> = output.as_os_str().encode_wide().chain(Some(0)).collect();
    let replacement: Vec<u16> = part.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: Both paths are owned, NUL-terminated UTF-16 buffers that remain alive
    // for the call. Optional pointer parameters are null as allowed by ReplaceFileW.
    let result = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_output_directory(output: &Path) -> std::io::Result<()> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_output_directory(_output: &Path) -> std::io::Result<()> {
    Ok(())
}

fn prepare_output(output: &Path, part: &Path, force: bool) -> Result<()> {
    if output.exists() && !force {
        bail!("refusing to overwrite {}; pass --force", output.display());
    }
    if part.exists() {
        if force {
            fs::remove_file(part)?;
        } else {
            bail!(
                "partial capture already exists at {}; pass --force",
                part.display()
            );
        }
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        bail!("output directory does not exist: {}", parent.display());
    }
    Ok(())
}

async fn fetch_precision(symbol: &str) -> Result<(u8, u8)> {
    let (mut socket, _) =
        tokio::time::timeout(Duration::from_secs(15), connect_async(KRAKEN_WEBSOCKET))
            .await
            .context("timed out connecting to Kraken")??;
    socket
        .send(Message::Text(
            json!({"method":"subscribe","params":{"channel":"instrument","snapshot":true},"req_id":1})
                .to_string()
                .into(),
        ))
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .context("timed out waiting for Kraken instrument metadata")?
            .context("Kraken closed the instrument stream")??;
        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_slice(text.as_bytes())?;
        let Some(pairs) = value
            .get("data")
            .and_then(|data| data.get("pairs"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        if let Some(pair) = pairs
            .iter()
            .find(|pair| pair.get("symbol").and_then(Value::as_str) == Some(symbol))
        {
            let price: u8 = pair
                .get("price_precision")
                .and_then(Value::as_u64)
                .and_then(|value| value.try_into().ok())
                .context("instrument is missing price_precision")?;
            let quantity: u8 = pair
                .get("qty_precision")
                .and_then(Value::as_u64)
                .and_then(|value| value.try_into().ok())
                .context("instrument is missing qty_precision")?;
            return Ok((price, quantity));
        }
    }
}

fn marker(started: Instant, kind: RecordKind, value: Value) -> Result<TapeRecord> {
    Ok(TapeRecord {
        receive_offset_ns: started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX),
        kind,
        payload: serde_json::to_vec(&value)?,
    })
}

fn write_marker(
    writer: &mut TapeWriter<fs::File>,
    validator: &mut ReplaySession,
    started: Instant,
    kind: RecordKind,
    value: Value,
) -> Result<()> {
    let record = marker(started, kind, value)?;
    validator.apply_record(&record)?;
    writer.push(record)?;
    Ok(())
}

fn should_stop(deadline: Option<Instant>, shutdown: &watch::Receiver<bool>) -> bool {
    *shutdown.borrow() || deadline.is_some_and(|deadline| Instant::now() >= deadline)
}

async fn wait_or_stop(
    duration: Duration,
    deadline: Option<Instant>,
    shutdown: &mut watch::Receiver<bool>,
) {
    let remaining = deadline
        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
        .unwrap_or(duration);
    tokio::select! {
        _ = tokio::time::sleep(duration.min(remaining)) => {}
        _ = shutdown.changed() => {}
    }
}

#[cfg(test)]
mod tests {
    use super::finalize_output;

    #[test]
    fn forced_finalization_replaces_the_destination() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("capture.ddt");
        let part = directory.path().join("capture.ddt.part");
        std::fs::write(&output, b"old").unwrap();
        std::fs::write(&part, b"new").unwrap();

        finalize_output(&part, &output, true).unwrap();

        assert_eq!(std::fs::read(output).unwrap(), b"new");
        assert!(!part.exists());
    }

    #[test]
    fn unforced_finalization_preserves_a_racing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("capture.ddt");
        let part = directory.path().join("capture.ddt.part");
        std::fs::write(&output, b"racing writer").unwrap();
        std::fs::write(&part, b"new capture").unwrap();

        finalize_output(&part, &output, false).unwrap_err();

        assert_eq!(std::fs::read(output).unwrap(), b"racing writer");
        assert_eq!(std::fs::read(part).unwrap(), b"new capture");
    }
}
