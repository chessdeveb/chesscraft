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
  lines,
  error,
  onStart,
  onStop,
  onPause,
  onResume,
  onClose,
}: EnginePanelProps) {
  async function handleBrowse() {
    const path = await open({
      multiple: false,
      filters: [{ name: "Exécutable", extensions: ["exe"] }],
    });
    if (path) onEnginePathChange(path);
  }

  return (
    <div className="engine-panel">
      <div className="engine-config">
        <input
          value={enginePath}
          onChange={(e) => onEnginePathChange(e.currentTarget.value)}
          placeholder="Chemin vers Stockfish (.exe)"
          disabled={running}
        />
        <button onClick={handleBrowse} disabled={running}>
          Parcourir
        </button>
        <button onClick={onClose}>Fermer</button>
      </div>

      <div className="engine-config">
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
        {!running ? (
          <button onClick={onStart} disabled={starting || !enginePath}>
            {starting ? "Démarrage…" : "Démarrer"}
          </button>
        ) : (
          <>
            <button onClick={onStop}>Arrêter</button>
            {paused ? (
              <button onClick={onResume}>Reprendre</button>
            ) : (
              <button onClick={onPause}>Pause</button>
            )}
          </>
        )}
      </div>

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
