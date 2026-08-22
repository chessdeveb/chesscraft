import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { EngineInfo } from "../types";

interface EnginePanelProps {
  enginePath: string;
  onEnginePathChange: (path: string) => void;
  running: boolean;
  starting: boolean;
  paused: boolean;
  depth: number;
  onDepthChange: (depth: number) => void;
  multiPv: number;
  onMultiPvChange: (multiPv: number) => void;
  threads: number;
  onThreadsChange: (threads: number) => void;
  maxThreads: number;
  lines: EngineInfo[];
  error: string | null;
  onStart: () => void;
  onStop: () => void;
  onPause: () => void;
  onResume: () => void;
  onClose: () => void;
}

function formatScore(info: EngineInfo): string {
  if (info.scoreMate !== null) {
    return `#${info.scoreMate}`;
  }
  if (info.scoreCp !== null) {
    const value = (info.scoreCp / 100).toFixed(2);
    return info.scoreCp > 0 ? `+${value}` : value;
  }
  return "…";
}

export function EnginePanel({
  enginePath,
  onEnginePathChange,
  running,
  starting,
  paused,
  depth,
  onDepthChange,
  multiPv,
  onMultiPvChange,
  threads,
  onThreadsChange,
  maxThreads,
  lines,
  error,
  onStart,
  onStop,
  onPause,
  onResume,
  onClose,
}: EnginePanelProps) {
  const [showSettings, setShowSettings] = useState(() => !enginePath);

  async function handleBrowse() {
    const path = await open({
      multiple: false,
      filters: [{ name: "Exécutable", extensions: ["exe"] }],
    });
    if (path) onEnginePathChange(path);
  }

  return (
    <div className="engine-panel">
      <div className="engine-toolbar">
        {!running ? (
          <button onClick={onStart} disabled={starting || !enginePath} title="Démarrer" aria-label="Démarrer">
            {starting ? "…" : "▶"}
          </button>
        ) : (
          <>
            <button onClick={onStop} title="Arrêter" aria-label="Arrêter">
              ⏹
            </button>
            {paused ? (
              <button onClick={onResume} title="Reprendre" aria-label="Reprendre">
                ▶
              </button>
            ) : (
              <button onClick={onPause} title="Pause" aria-label="Pause">
                ⏸
              </button>
            )}
          </>
        )}
        <button
          onClick={() => setShowSettings((v) => !v)}
          title="Paramètres du moteur"
          aria-label="Paramètres du moteur"
          className={"icon-toggle" + (showSettings ? " icon-toggle--active" : "")}
        >
          ⚙
        </button>
        <button onClick={onClose} title="Fermer le panneau" aria-label="Fermer le panneau">
          ✕
        </button>
      </div>

      {showSettings && (
        <div className="engine-settings">
          <label className="engine-field engine-field--full">
            Chemin Stockfish
            <div className="engine-path-row">
              <input
                value={enginePath}
                onChange={(e) => onEnginePathChange(e.currentTarget.value)}
                placeholder="Chemin vers Stockfish (.exe)"
                disabled={running}
              />
              <button onClick={handleBrowse} disabled={running}>
                Parcourir
              </button>
            </div>
          </label>
          <div className="engine-settings-row">
            <label className="engine-field">
              Profondeur
              <select value={depth} onChange={(e) => onDepthChange(Number(e.currentTarget.value))}>
                <option value={10}>10</option>
                <option value={14}>14</option>
                <option value={18}>18</option>
                <option value={22}>22</option>
                <option value={26}>26</option>
              </select>
            </label>
            <label className="engine-field">
              Variantes
              <select value={multiPv} onChange={(e) => onMultiPvChange(Number(e.currentTarget.value))}>
                <option value={1}>1</option>
                <option value={2}>2</option>
                <option value={3}>3</option>
                <option value={4}>4</option>
                <option value={5}>5</option>
              </select>
            </label>
            <label className="engine-field">
              Cœurs
              <select value={threads} onChange={(e) => onThreadsChange(Number(e.currentTarget.value))}>
                {Array.from({ length: maxThreads }, (_, i) => i + 1).map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </div>
      )}

      {error && <p className="error">{error}</p>}

      {running && (
        <div className="engine-lines">
          {paused && <p className="engine-line engine-line--paused">En pause</p>}
          {lines.length === 0 && !paused && <p className="engine-line">Analyse en cours…</p>}
          {lines.map((info) => (
            <div key={info.multipv} className="engine-result">
              <span className="engine-score">{formatScore(info)}</span>
              <span className="engine-depth">profondeur {info.depth}</span>
              <p className="engine-line">{info.pvSan.join(" ")}</p>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
