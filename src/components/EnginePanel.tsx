import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { EngineInfo } from "../types";

interface EnginePanelProps {
  fen: string;
}

function formatScore(info: EngineInfo): string {
  if (info.scoreMate !== null) {
    return info.scoreMate > 0 ? `#${info.scoreMate}` : `#${info.scoreMate}`;
  }
  if (info.scoreCp !== null) {
    const value = (info.scoreCp / 100).toFixed(2);
    return info.scoreCp > 0 ? `+${value}` : value;
  }
  return "…";
}

export function EnginePanel({ fen }: EnginePanelProps) {
  const [enginePath, setEnginePath] = useState("");
  const [running, setRunning] = useState(false);
  const [starting, setStarting] = useState(false);
  const [depth, setDepth] = useState(18);
  const [info, setInfo] = useState<EngineInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  // le moteur s'arrete automatiquement si le panneau se referme/demonte
  useEffect(() => {
    return () => {
      invoke("stop_engine").catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!running) return;
    const unlistenPromise = listen<EngineInfo>("engine-info", (event) => {
      setInfo(event.payload);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [running]);

  useEffect(() => {
    if (!running) return;
    setInfo(null);
    invoke("analyze_position", { fen, depth }).catch((e) => setError(String(e)));
  }, [fen, running, depth]);

  async function handleBrowse() {
    const path = await open({
      multiple: false,
      filters: [{ name: "Exécutable", extensions: ["exe"] }],
    });
    if (path) setEnginePath(path);
  }

  async function handleToggle() {
    if (running) {
      await invoke("stop_engine").catch((e) => setError(String(e)));
      setRunning(false);
      setInfo(null);
      return;
    }
    setStarting(true);
    try {
      await invoke("start_engine", { path: enginePath });
      setRunning(true);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  }

  return (
    <div className="engine-panel">
      <div className="engine-config">
        <input
          value={enginePath}
          onChange={(e) => setEnginePath(e.currentTarget.value)}
          placeholder="Chemin vers Stockfish (.exe)"
          disabled={running}
        />
        <button onClick={handleBrowse} disabled={running}>
          Parcourir
        </button>
        <select
          value={depth}
          onChange={(e) => setDepth(Number(e.currentTarget.value))}
        >
          <option value={12}>Profondeur 12</option>
          <option value={18}>Profondeur 18</option>
          <option value={24}>Profondeur 24</option>
        </select>
        <button onClick={handleToggle} disabled={starting || (!running && !enginePath)}>
          {running ? "Arrêter" : starting ? "Démarrage…" : "Démarrer"}
        </button>
      </div>
      {error && <p className="error">{error}</p>}
      {running && (
        <div className="engine-result">
          {info ? (
            <>
              <span className="engine-score">{formatScore(info)}</span>
              <span className="engine-depth">profondeur {info.depth}</span>
              <p className="engine-line">{info.pvSan.join(" ")}</p>
            </>
          ) : (
            <p className="engine-line">Analyse en cours…</p>
          )}
        </div>
      )}
    </div>
  );
}
