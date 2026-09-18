import { useState } from "react";
import { createPortal } from "react-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";
import { Board } from "./Board";
import type { EngineInfo, PvMove } from "../types";

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
  /** FEN de la position analysee : sert uniquement a numeroter les coups de
      la ligne principale (trait et numero de coup de depart). */
  analyzedFen: string | null;
  boardTheme: string;
  /** Orientation de l'echiquier principal (voir AppContext::boardFlipped) --
      l'apercu au survol d'une ligne doit s'afficher dans le meme sens, pas
      systematiquement Blancs en bas. */
  orientation: Color;
  onStart: () => void;
  onStop: () => void;
  onPause: () => void;
  onResume: () => void;
  onClose: () => void;
}

const NO_DESTS: Dests = new Map();

interface HoverPreview {
  fen: string;
  from: string;
  to: string;
  x: number;
  y: number;
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

/** Trait et numero de coup de depart, lus directement dans le FEN analyse
    (champs 2 et 6) -- evite de dupliquer cote JS une logique d'echecs deja
    geree cote Rust. */
function fenMoveContext(fen: string | null): { whiteToMove: boolean; moveNumber: number } {
  const parts = (fen ?? "").split(" ");
  const whiteToMove = parts[1] !== "b";
  const moveNumber = Number(parts[5]) || 1;
  return { whiteToMove, moveNumber };
}

// onLeave n'est volontairement PAS pose sur chaque coup individuellement :
// passer d'un coup au suivant traverse un espace non couvert par les spans
// (texte brut entre les coups), donc un onMouseLeave par coup declenchait un
// aller-retour disparition/reapparition de l'apercu a chaque changement de
// coup survole. Un seul onMouseLeave sur tout le conteneur de la ligne (voir
// l'appelant) : ne se declenche qu'en quittant reellement toute la variante.
function renderPv(pv: PvMove[], analyzedFen: string | null, onHover: (move: PvMove, e: React.MouseEvent) => void) {
  const { whiteToMove, moveNumber } = fenMoveContext(analyzedFen);
  let number = moveNumber;
  let isWhite = whiteToMove;
  const out: React.ReactNode[] = [];
  pv.forEach((move, i) => {
    if (isWhite) {
      out.push(
        <span className="move-number" key={`n${i}`}>
          {number}.{" "}
        </span>
      );
    } else if (i === 0) {
      out.push(
        <span className="move-number" key={`n${i}`}>
          {number}...{" "}
        </span>
      );
    }
    out.push(
      <span className="engine-pv-move" key={i} onMouseEnter={(e) => onHover(move, e)}>
        {move.san}
      </span>,
      " "
    );
    if (!isWhite) number += 1;
    isWhite = !isWhite;
  });
  return out;
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
  analyzedFen,
  boardTheme,
  orientation,
  onStart,
  onStop,
  onPause,
  onResume,
  onClose,
}: EnginePanelProps) {
  const { t } = useTranslation();
  const [showSettings, setShowSettings] = useState(() => !enginePath);
  const [hover, setHover] = useState<HoverPreview | null>(null);

  async function handleBrowse() {
    const path = await open({
      multiple: false,
      filters: [{ name: t("engine.executableFilter"), extensions: ["exe"] }],
    });
    if (path) onEnginePathChange(path);
  }

  // Taille fixe du petit echiquier (voir .engine-hover-preview dans
  // App.css) : sert a le garder entierement a l'ecran (voir clamp ci-dessous).
  const PREVIEW_SIZE = 200;
  const EDGE_MARGIN = 8;

  function handleMoveHover(move: PvMove, e: React.MouseEvent) {
    const rect = e.currentTarget.getBoundingClientRect();
    let x = rect.left;
    let y = rect.bottom;
    if (x + PREVIEW_SIZE + EDGE_MARGIN > window.innerWidth) x = window.innerWidth - PREVIEW_SIZE - EDGE_MARGIN;
    if (y + PREVIEW_SIZE + EDGE_MARGIN > window.innerHeight) y = rect.top - PREVIEW_SIZE;
    x = Math.max(EDGE_MARGIN, x);
    y = Math.max(EDGE_MARGIN, y);
    setHover({ fen: move.fen, from: move.from, to: move.to, x, y });
  }

  return (
    <div className="engine-panel">
      <div className="engine-toolbar">
        {!running ? (
          <button onClick={onStart} disabled={starting || !enginePath} title={t("engine.start")} aria-label={t("engine.start")}>
            {starting ? "…" : "▶"}
          </button>
        ) : (
          <>
            <button onClick={onStop} title={t("engine.stop")} aria-label={t("engine.stop")}>
              ⏹
            </button>
            {paused ? (
              <button onClick={onResume} title={t("engine.resume")} aria-label={t("engine.resume")}>
                ▶
              </button>
            ) : (
              <button onClick={onPause} title={t("engine.pause")} aria-label={t("engine.pause")}>
                ⏸
              </button>
            )}
          </>
        )}
        <button
          onClick={() => setShowSettings((v) => !v)}
          title={t("engine.settings")}
          aria-label={t("engine.settings")}
          className={"icon-toggle" + (showSettings ? " icon-toggle--active" : "")}
        >
          ⚙
        </button>
        <button onClick={onClose} title={t("engine.closePanel")} aria-label={t("engine.closePanel")}>
          ✕
        </button>
      </div>

      {showSettings && (
        <div className="engine-settings">
          <label className="engine-field engine-field--full">
            {t("engine.stockfishPath")}
            <div className="engine-path-row">
              <input
                value={enginePath}
                onChange={(e) => onEnginePathChange(e.currentTarget.value)}
                placeholder={t("engine.stockfishPathPlaceholder")}
                disabled={running}
              />
              <button onClick={handleBrowse} disabled={running}>
                {t("engine.browse")}
              </button>
            </div>
          </label>
          <div className="engine-settings-row">
            <label className="engine-field">
              {t("engine.depth")}
              <select value={depth} onChange={(e) => onDepthChange(Number(e.currentTarget.value))}>
                <option value={10}>10</option>
                <option value={14}>14</option>
                <option value={18}>18</option>
                <option value={22}>22</option>
                <option value={26}>26</option>
              </select>
            </label>
            <label className="engine-field">
              {t("engine.multiPv")}
              <select value={multiPv} onChange={(e) => onMultiPvChange(Number(e.currentTarget.value))}>
                <option value={1}>1</option>
                <option value={2}>2</option>
                <option value={3}>3</option>
                <option value={4}>4</option>
                <option value={5}>5</option>
              </select>
            </label>
            <label className="engine-field">
              {t("engine.threads")}
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
          {paused && <p className="engine-line engine-line--paused">{t("engine.paused")}</p>}
          {lines.length === 0 && !paused && <p className="engine-line">{t("engine.analyzing")}</p>}
          {lines.map((info) => (
            <div key={info.multipv} className="engine-result">
              <span className="engine-score">{formatScore(info)}</span>
              <span className="engine-line" onMouseLeave={() => setHover(null)}>
                {renderPv(info.pv, analyzedFen, handleMoveHover)}
              </span>
            </div>
          ))}
        </div>
      )}

      {hover &&
        createPortal(
          // Portail direct dans <body> : dockview place transform/contain sur
          // ses conteneurs de panneaux (optimisation de rendu), ce qui cree
          // un nouveau bloc englobant pour tout descendant en position:fixed
          // -- sans portail, l'apercu restait piege dans les limites du
          // panneau Moteur au lieu du viewport entier, invisible des qu'il
          // aurait du deborder par-dessus un autre panneau.
          <div className="engine-hover-preview" style={{ left: hover.x, top: hover.y }}>
            <Board
              fen={hover.fen}
              turnColor={(hover.fen.split(" ")[1] === "b" ? "black" : "white") as Color}
              dests={NO_DESTS}
              shapes={[]}
              theme={boardTheme}
              lastMove={hover.from && hover.to ? [hover.from as Key, hover.to as Key] : null}
              orientation={orientation}
              onMove={() => {}}
              onShapesChange={() => {}}
            />
          </div>,
          document.body
        )}
    </div>
  );
}
