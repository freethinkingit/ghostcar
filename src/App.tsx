import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

interface ProgressEvent {
  current: number;
  total: number;
  file: string;
  status: string;
  percent: number;
}

interface StatusCounts {
  total: number;
  done: number;
  failed: number;
  pending: number;
  invalid: number;
}

function App() {
  const [source, setSource] = useState("");
  const [dest, setDest] = useState("");
  const [counts, setCounts] = useState<StatusCounts | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [log, setLog] = useState<ProgressEvent[]>([]);
  const [result, setResult] = useState("");

  useEffect(() => {
    const unlisten = listen<ProgressEvent>("progress", (event) => {
      setProgress(event.payload);
      setLog((prev) => [event.payload, ...prev].slice(0, 100));
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  const pickSource = async () => {
    const dir = await open({ directory: true, title: "Select Source Folder (Originals)" });
    if (dir) {
      setSource(dir as string);
      await invoke("set_source", { dir });
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
    }
  };

  const pickDest = async () => {
    const dir = await open({ directory: true, title: "Select Destination Folder (Proxies)" });
    if (dir) {
      setDest(dir as string);
      await invoke("set_dest", { dir });
    }
  };

  const startEncode = async () => {
    setRunning(true);
    setLog([]);
    setResult("");
    try {
      const res = await invoke<string>("start", {});
      setResult(res);
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
    } catch (e) { setResult(`Error: ${e}`); }
    setRunning(false);
  };

  const runValidate = async () => {
    setRunning(true);
    setLog([]);
    setResult("");
    try {
      const res = await invoke<string>("validate", {});
      setResult(res);
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
    } catch (e) { setResult(`Error: ${e}`); }
    setRunning(false);
  };

  const runRetry = async () => {
    try {
      const res = await invoke<string>("retry", {});
      setResult(res);
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
    } catch (e) { setResult(`Error: ${e}`); }
  };

  const pct = progress ? progress.percent : 0;

  return (
    <div className="app">
      <header>
        <h1>🏎️ GhostCar</h1>
        <p className="subtitle">Lightweight proxy generator for DaVinci Resolve</p>
      </header>

      <section className="folders">
        <div className="folder-row">
          <button onClick={pickSource}>Source Folder</button>
          <span className="path">{source || "No folder selected"}</span>
        </div>
        <div className="folder-row">
          <button onClick={pickDest}>Destination Folder</button>
          <span className="path">{dest || "No folder selected"}</span>
        </div>
      </section>

      {counts && (
        <section className="status-counts">
          <span className="badge">{counts.total} total</span>
          <span className="badge done">{counts.done} done</span>
          <span className="badge pending">{counts.pending} pending</span>
          {counts.failed > 0 && <span className="badge failed">{counts.failed} failed</span>}
          {counts.invalid > 0 && <span className="badge invalid">{counts.invalid} invalid</span>}
        </section>
      )}

      <section className="controls">
        <button className="start-btn" onClick={startEncode} disabled={!source || !dest || running}>
          {running ? "Working..." : "Start"}
        </button>
        <button onClick={runValidate} disabled={!source || !dest || running}>Validate</button>
        <button onClick={runRetry} disabled={!source || running || (!counts?.failed && !counts?.invalid)}>
          Retry
        </button>
      </section>

      {progress && (
        <section className="progress">
          <div className="progress-bar">
            <div className="progress-fill" style={{ width: `${pct}%` }} />
          </div>
          <p className="progress-text">
            [{progress.current}/{progress.total}] {progress.file}
            <span className={`status ${progress.status}`}> {progress.status} {pct}%</span>
          </p>
        </section>
      )}

      {result && <section className="result">{result}</section>}

      {log.length > 0 && (
        <section className="log">
          {log.map((entry, i) => (
            <div key={i} className={`log-entry ${entry.status}`}>
              [{entry.current}/{entry.total}] {entry.file} — {entry.status}
            </div>
          ))}
        </section>
      )}
    </div>
  );
}

export default App;
