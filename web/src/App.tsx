import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BookCanvas } from "./BookCanvas";
import { formatDuration } from "./time";
import type { ReplayFrame, TapeInfo, WorkerRequest, WorkerResponse } from "./types";

const speeds = ["0.1", "1", "10", "max"] as const;

export default function App() {
  const workerRef = useRef<Worker | null>(null);
  const animationRef = useRef<number | null>(null);
  const lastTickRef = useRef<number | null>(null);
  const elapsedRef = useRef(0);
  const durationRef = useRef(0);
  const speedRef = useRef<(typeof speeds)[number]>("1");
  const [frame, setFrame] = useState<ReplayFrame | null>(null);
  const [info, setInfo] = useState<TapeInfo | null>(null);
  const [digest, setDigest] = useState("—");
  const [elapsedMs, setElapsedMs] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState<(typeof speeds)[number]>("1");
  const [source, setSource] = useState("no tape loaded");
  const [error, setError] = useState<string | null>(null);
  const [events, setEvents] = useState<ReplayFrame[]>([]);

  const loadBytes = useCallback((bytes: ArrayBuffer, name: string) => {
    setPlaying(false);
    setError(null);
    setSource(name);
    setInfo(null);
    setFrame(null);
    setDigest("—");
    setElapsedMs(0);
    elapsedRef.current = 0;
    durationRef.current = 0;
    setEvents([]);
    workerRef.current?.postMessage({ type: "load", bytes } satisfies WorkerRequest, [bytes]);
  }, []);

  useEffect(() => {
    const worker = new Worker(new URL("./replay.worker.ts", import.meta.url), { type: "module" });
    workerRef.current = worker;
    worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
      const response = event.data;
      if (response.type === "error") {
        setError(response.message);
        setPlaying(false);
        return;
      }
      if (response.type === "loaded") {
        setInfo(response.info);
        durationRef.current = response.info.duration_ms;
        elapsedRef.current = response.frame.elapsed_ns / 1e6;
        setElapsedMs(elapsedRef.current);
        setEvents([]);
      }
      setFrame(response.frame);
      setDigest(response.digest);
      setEvents((current) => {
        if (current[0]?.messages === response.frame.messages) return current;
        return [response.frame, ...current].slice(0, 6);
      });
    };
    return () => {
      worker.terminate();
      if (animationRef.current !== null) cancelAnimationFrame(animationRef.current);
    };
  }, [loadBytes]);

  useEffect(() => {
    speedRef.current = speed;
  }, [speed]);

  useEffect(() => {
    if (!playing) {
      lastTickRef.current = null;
      if (animationRef.current !== null) cancelAnimationFrame(animationRef.current);
      return;
    }
    const tick = (now: number) => {
      const previous = lastTickRef.current ?? now;
      lastTickRef.current = now;
      const selected = speedRef.current;
      const next = selected === "max"
        ? durationRef.current
        : Math.min(durationRef.current, elapsedRef.current + (now - previous) * Number(selected));
      elapsedRef.current = next;
      setElapsedMs(next);
      workerRef.current?.postMessage({ type: "advance", elapsedMs: next } satisfies WorkerRequest);
      if (next >= durationRef.current) {
        setPlaying(false);
        return;
      }
      animationRef.current = requestAnimationFrame(tick);
    };
    animationRef.current = requestAnimationFrame(tick);
    return () => {
      if (animationRef.current !== null) cancelAnimationFrame(animationRef.current);
    };
  }, [playing]);

  const seek = (next: number) => {
    setPlaying(false);
    elapsedRef.current = next;
    setElapsedMs(next);
    workerRef.current?.postMessage({ type: "seek", elapsedMs: next } satisfies WorkerRequest);
  };

  const loadFile = async (file: File) => {
    if (!file.name.endsWith(".ddt")) {
      setError("DepthDeck tapes use the .ddt extension");
      return;
    }
    loadBytes(await file.arrayBuffer(), file.name);
  };

  const throughput = useMemo(() => {
    if (!frame || frame.elapsed_ns === 0) return 0;
    return frame.price_level_mutations / (frame.elapsed_ns / 1e9);
  }, [frame]);

  return (
    <main
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        const file = event.dataTransfer.files[0];
        if (file) void loadFile(file);
      }}
    >
      <header className="masthead">
        <div>
          <p className="eyebrow">DETERMINISTIC MARKET REPLAY</p>
          <h1>Depth<span>Deck</span></h1>
        </div>
        <div className="header-actions">
          <span className={`source-badge ${info ? "local" : "empty"}`}>{info ? "LOCAL TAPE" : "NO TAPE"}</span>
          <label className="file-button">
            Load .ddt
            <input type="file" accept=".ddt" onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) void loadFile(file);
            }} />
          </label>
          <a href="https://github.com/ericskavinski/depthdeck">GitHub ↗</a>
        </div>
      </header>

      {error && <div className="error" role="alert">{error}</div>}

      <section className="ticker-strip" aria-label="Replay status">
        <Stat label="SYMBOL" value={info?.metadata.symbol ?? "—"} />
        <Stat label="SPREAD" value={frame?.spread ?? "—"} accent />
        <Stat label="CHECKSUM" value={frame?.checksum_valid ? "VALID" : "WAITING"} good={frame?.checksum_valid} />
        <Stat label="BOOK STATE" value={frame?.synchronized ? "SYNCHRONIZED" : "UNSYNCED"} good={frame?.synchronized} />
        <Stat label="DIGEST" value={digest} mono />
      </section>

      <section className="workspace">
        <article className="panel order-book">
          <PanelHeading index="01" title="RECONSTRUCTED BOOK" detail={`${info?.metadata.depth ?? 0} levels / side`} />
          <BookCanvas frame={frame} />
          <div className="best-levels">
            <span><i className="bid-dot" />BEST BID <b>{frame?.bids[0]?.price ?? "—"}</b></span>
            <span><i className="ask-dot" />BEST ASK <b>{frame?.asks[0]?.price ?? "—"}</b></span>
          </div>
        </article>

        <article className="panel trace-panel">
          <PanelHeading index="02" title="SYSTEMS TRACE" detail={source} />
          <div className="trace-grid">
            <Trace label="RECEIVE CLOCK" value={formatDuration(elapsedMs)} />
            <Trace label="EXCHANGE CLOCK" value={frame?.exchange_timestamp?.split("T")[1]?.replace("Z", "") ?? "—"} />
            <Trace label="MESSAGES APPLIED" value={(frame?.messages ?? 0).toLocaleString()} />
            <Trace label="LEVEL MUTATIONS" value={(frame?.price_level_mutations ?? 0).toLocaleString()} />
            <Trace label="AVG THROUGHPUT" value={`${Math.round(throughput).toLocaleString()} /s`} />
            <Trace label="TAPE CHUNKS" value={(info?.chunks ?? 0).toString()} />
            <Trace label="COMPRESSION" value={info ? `${(info.compression_ratio * 100).toFixed(1)}%` : "—"} />
            <Trace label="RECORDS" value={(info?.records ?? 0).toLocaleString()} />
          </div>
          <div className="histogram">
            <p>INTER-ARRIVAL DISTRIBUTION</p>
            <div className="bars">
              {(info?.inter_arrival_buckets ?? [0, 0, 0, 0, 0]).map((value, index, all) => (
                <div key={index} style={{ height: `${Math.max(4, value / Math.max(...all, 1) * 100)}%` }}><span>{["<100µ", "<1m", "<10m", "<100m", "100m+"][index]}</span></div>
              ))}
            </div>
          </div>
        </article>
      </section>

      <section className="panel transport">
        <PanelHeading index="03" title="REPLAY TRANSPORT" detail="monotonic receive time" />
        <div className="transport-row">
          <button className="play" onClick={() => setPlaying((value) => !value)} disabled={!info} aria-label={playing ? "Pause replay" : "Play replay"}>
            {playing ? "Ⅱ" : "▶"}
          </button>
          <span className="clock">{formatDuration(elapsedMs)}</span>
          <input
            aria-label="Replay position"
            type="range"
            min="0"
            max={Math.max(info?.duration_ms ?? 1, 1)}
            step="1"
            value={elapsedMs}
            onChange={(event) => seek(Number(event.target.value))}
          />
          <span className="clock dim">{formatDuration(info?.duration_ms ?? 0)}</span>
          <div className="speed-group" aria-label="Playback speed">
            {speeds.map((item) => <button key={item} className={speed === item ? "active" : ""} onClick={() => setSpeed(item)}>{item === "max" ? "MAX" : `${item}×`}</button>)}
          </div>
        </div>
      </section>

      <section className="panel event-panel">
        <PanelHeading index="04" title="EVENT INSPECTOR" detail="latest normalized + raw wire record" />
        <div className="event-grid">
          <div className="event-list">
            {events.length === 0 && <p className="empty">Advance the tape to inspect events.</p>}
            {events.map((event) => (
              <button key={`${event.elapsed_ns}-${event.messages}`} onClick={() => seek(event.elapsed_ns / 1e6)}>
                <span>{event.last_record_kind ?? "Record"}</span>
                <b>{formatDuration(event.elapsed_ns / 1e6)}</b>
                <small>msg {event.messages}</small>
              </button>
            ))}
          </div>
          <pre>{frame?.last_payload ? prettyJson(frame.last_payload) : "// no wire payload selected"}</pre>
        </div>
      </section>

      <footer>
        <span>Rust core · WebAssembly replay · CRC32 verified</span>
        <span>Local tape processing stays in this browser. No trading functionality.</span>
      </footer>
    </main>
  );
}

function Stat({ label, value, accent, good, mono }: { label: string; value: string; accent?: boolean; good?: boolean; mono?: boolean }) {
  return <div className={`stat ${accent ? "accent" : ""} ${good ? "good" : ""} ${mono ? "mono" : ""}`}><small>{label}</small><strong>{value}</strong></div>;
}

function Trace({ label, value }: { label: string; value: string }) {
  return <div><small>{label}</small><strong>{value}</strong></div>;
}

function PanelHeading({ index, title, detail }: { index: string; title: string; detail: string }) {
  return <div className="panel-heading"><span>{index}</span><h2>{title}</h2><small>{detail}</small></div>;
}

function prettyJson(payload: string) {
  try { return JSON.stringify(JSON.parse(payload), null, 2); } catch { return payload; }
}
