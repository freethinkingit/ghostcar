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
}

function App() {
  const [source, setSource] = useState("");
  const [dest, setDest] = useState("");
  const [fileCount, setFileCount] = useState<number | null>(null);
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
      const count = await invoke<number>("scan", {});
      setFileCount(count);
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
    } catch (e) {
      setResult(`Error: ${e}`);
    }
    setRunning(false);
  };

  const pct = progress ? Math.round((progress.current / progress.total) * 100) : 0;

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
          {fileCount !== null && <span className="badge">{fileCount} files</span>}
        </div>
        <div className="folder-row">
          <button onClick={pickDest}>Destination Folder</button>
          <span className="path">{dest || "No folder selected"}</span>
        </div>
      </section>

      <section className="controls">
        <button
          className="start-btn"
          onClick={startEncode}
          disabled={!source || !dest || running}
        >
          {running ? "Encoding..." : "Start"}
        </button>
      </section>

      {progress && (
        <section className="progress">
          <div className="progress-bar">
            <div className="progress-fill" style={{ width: `${pct}%` }} />
          </div>
          <p className="progress-text">
            {progress.current}/{progress.total} — {progress.file}
            <span className={`status ${progress.status}`}> {progress.status}</span>
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
