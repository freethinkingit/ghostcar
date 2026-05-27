import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

interface ProgressEvent {
  file: string;
  status: string;
  percent: number;
  done_count: number;
  failed_count: number;
  total: number;
}

interface StatusCounts {
  total: number;
  done: number;
  failed: number;
  pending: number;
  invalid: number;
}

interface FileState {
  status: string;
  percent: number;
}

interface HwInfo {
  chip: string;
  workers: number;
  has_videotoolbox: boolean;
}

function App() {
  const [source, setSource] = useState("");
  const [dest, setDest] = useState("");
  const [counts, setCounts] = useState<StatusCounts | null>(null);
  const [running, setRunning] = useState(false);
  const [files, setFiles] = useState<Map<string, FileState>>(new Map());
  const [doneCount, setDoneCount] = useState(0);
  const [failedCount, setFailedCount] = useState(0);
  const [totalCount, setTotalCount] = useState(0);
  const [result, setResult] = useState("");
  const [hw, setHw] = useState<HwInfo | null>(null);
  const [modal, setModal] = useState<{ title: string; body: string; onConfirm: () => void } | null>(null);
  const [startTime, setStartTime] = useState<number | null>(null);
  const [preparing, setPreparing] = useState(false);

  useEffect(() => { invoke<HwInfo>("hw_info", {}).then(setHw); }, []);

  useEffect(() => {
    const unlisten = listen<ProgressEvent>("progress", (event) => {
      const { file, status, percent, done_count, failed_count, total } = event.payload;
      setPreparing(false);
      setDoneCount(done_count);
      setFailedCount(failed_count);
      setTotalCount(total);
      setFiles((prev) => {
        const next = new Map(prev);
        next.set(file, { status, percent });
        return next;
      });
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  const refreshFiles = async () => {
    const list = await invoke<[string, string][]>("get_files", {});
    const m = new Map<string, FileState>();
    for (const [f, s] of list) m.set(f, { status: s, percent: s === "done" ? 100 : 0 });
    setFiles(m);
  };

  const pickSource = async () => {
    const dir = await open({ directory: true, title: "Select Source Folder" });
    if (dir) {
      setSource(dir as string);
      await invoke("set_source", { dir });
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
      if (dest) await refreshFiles();
    }
  };

  const pickDest = async () => {
    const dir = await open({ directory: true, title: "Select Proxy Destination" });
    if (dir) {
      setDest(dir as string);
      await invoke("set_dest", { dir });
      if (source) await refreshFiles();
    }
  };

  const startEncode = async () => {
    setRunning(true);
    setPreparing(true);
    setResult("");
    setStartTime(Date.now());
    try {
      const res = await invoke<string>("start", {});
      setResult(res);
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
      await refreshFiles();
    } catch (e) { setResult(`Error: ${e}`); }
    setRunning(false);
    setPreparing(false);
    setStartTime(null);
  };

  const stopEncode = async () => {
    await invoke("stop", {});
    setResult("Stopped. Remaining files are still pending.");
  };

  const runValidate = async () => {
    setRunning(true);
    setResult("");
    try {
      const res = await invoke<string>("validate", {});
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
      await refreshFiles();
      const invalidCount = (c.failed ?? 0) + (c.invalid ?? 0);
      if (invalidCount > 0) {
        setModal({
          title: `${invalidCount} file${invalidCount > 1 ? "s" : ""} failed validation`,
          body: `Would you like to queue ${invalidCount === 1 ? "it" : "them"} for re-encoding? Hit Start after to process.`,
          onConfirm: async () => {
            await invoke("retry", {});
            const c2 = await invoke<StatusCounts>("scan", {});
            setCounts(c2);
            await refreshFiles();
            setModal(null);
            setResult(`${invalidCount} file${invalidCount > 1 ? "s" : ""} queued. Hit Start to encode.`);
          },
        });
      } else {
        setResult(res);
      }
    } catch (e) { setResult(`Error: ${e}`); }
    setRunning(false);
  };

  const runRetry = async () => {
    try {
      const res = await invoke<string>("retry", {});
      setResult(res + " — hit Start to encode.");
      const c = await invoke<StatusCounts>("scan", {});
      setCounts(c);
      await refreshFiles();
    } catch (e) { setResult(`Error: ${e}`); }
  };

  const overallPct = totalCount > 0 ? Math.round(((doneCount + failedCount) / totalCount) * 100) : 0;
  const failCount = (counts?.failed ?? 0) + (counts?.invalid ?? 0);
  const encodingCount = Array.from(files.values()).filter(f => f.status === "encoding").length;
  const bothSelected = source && dest;

  const formatEta = () => {
    if (!startTime || doneCount === 0 || overallPct >= 100) return "";
    const elapsed = (Date.now() - startTime) / 1000;
    const rate = doneCount / elapsed; // files per second
    const remaining = totalCount - doneCount - failedCount;
    if (rate <= 0 || remaining <= 0) return "";
    const etaSec = remaining / rate;
    if (etaSec < 60) return `~${Math.round(etaSec)}s left`;
    if (etaSec < 3600) return `~${Math.round(etaSec / 60)}m left`;
    return `~${(etaSec / 3600).toFixed(1)}h left`;
  };

  // Guided empty state
  const emptyMessage = !source
    ? "① Select your source folder — where your original footage lives."
    : !dest
    ? "② Now select your proxy folder — where lightweight copies will go."
    : counts?.pending
    ? `${counts.pending} file${counts.pending > 1 ? "s" : ""} ready to encode. Hit Start →`
    : counts?.done
    ? "All files encoded. Use Validate to check integrity."
    : "Ready.";

  return (
    <div className="app">
      <div className="toolbar">
        <div className="toolbar-left">
          <div className="folder-row" onClick={pickSource} title="Your original high-res footage (e.g. external HDD)">
            <span className="label">SOURCE</span>
            <span className="path">{source || "Click to select..."}</span>
          </div>
          <div className="folder-row" onClick={pickDest} title="Where proxy files will be saved (e.g. fast internal SSD)">
            <span className="label">PROXIES</span>
            <span className="path">{dest || "Click to select..."}</span>
          </div>
        </div>
        <div className="toolbar-actions">
          <button
            onClick={runValidate}
            disabled={!bothSelected || running}
            title="Check all proxy files are valid and playable"
          >
            Validate
          </button>
          {failCount > 0 && (
            <button
              onClick={runRetry}
              disabled={running}
              title="Queue failed files for re-encoding, then hit Start"
            >
              Retry {failCount}
            </button>
          )}
          {running ? (
            <button className="stop" onClick={stopEncode} title="Stop encoding after current files finish">
              Stop
            </button>
          ) : (
            <button
              className="primary"
              onClick={startEncode}
              disabled={!bothSelected || !counts?.pending}
              title="Encode all pending files using hardware acceleration"
            >
              Start
            </button>
          )}
        </div>
      </div>

      <div className="file-list">
        {preparing && (
          <div className="preparing">
            <div className="spinner"></div>
            <span>Preparing encoder...</span>
          </div>
        )}
        {!preparing && (!bothSelected || files.size === 0) && (
          <div className="empty">{emptyMessage}</div>
        )}
        {!preparing && bothSelected && Array.from(files.entries()).map(([name, f]) => (
          <div key={name} className={`file-row ${f.status}`}>
            <span className="icon">
              {f.status === "done" ? "✓" : f.status === "encoding" || f.status === "validating" ? "⟳" : f.status === "failed" || f.status === "invalid" ? "✗" : "·"}
            </span>
            <span className="filename">{name}</span>
            {f.status === "encoding" ? (
              <div className="file-progress-wrap">
                <div className="file-progress">
                  <div className="file-progress-fill" style={{ width: `${f.percent}%` }} />
                </div>
                <span className="file-pct">{f.percent}%</span>
              </div>
            ) : (
              <span className="file-status-label">
                {f.status === "done" ? "completed" : f.status}
              </span>
            )}
          </div>
        ))}
      </div>

      <div className="status-bar">
        {hw && !hw.has_videotoolbox && (
          <div className="warning">⚠ No hardware encoder detected. Encoding will use software (much slower).</div>
        )}
        {running && totalCount > 0 && (
          <div className="progress-track">
            <div className="progress-fill" style={{ width: `${overallPct}%` }} />
          </div>
        )}
        <div className="status-info">
          {counts && (
            <span className="status-text">
              {counts.done} done · {counts.pending} pending
              {encodingCount > 0 && ` · ${encodingCount} encoding`}
              {failCount > 0 && ` · ${failCount} failed`}
            </span>
          )}
          {running && totalCount > 0 && (<>
            <span className="status-pct">{doneCount}/{totalCount} — {overallPct}%</span>
            {formatEta() && <span className="status-eta">{formatEta()}</span>}
          </>)}
          {result && !running && <span className="status-result">{result}</span>}
          {hw && !running && !result && (
            <span className="status-hw">{hw.chip} · {hw.workers} workers</span>
          )}
        </div>
      </div>

      {modal && (
        <div className="modal-overlay" onClick={() => setModal(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h3>{modal.title}</h3>
            <p>{modal.body}</p>
            <div className="modal-actions">
              <button onClick={() => setModal(null)}>No, leave them</button>
              <button className="primary" onClick={modal.onConfirm}>Yes, retry</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
