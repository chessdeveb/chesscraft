import { useEffect, useState } from "react";
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
  /** Table de hachage du moteur, en Mo. */
  hashMb: number;
  onHashChange: (hashMb: number) => void;
  /** Plafond en Mo (75 % de la memoire du PC), null s'il est inconnu. */
  maxHashMb: number | null;
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
  onPlayPvLine: (moves: PvMove[], uptoIndex: number) => void;
  onStart: () => void;
  onStop: () => void;
  onPause: () => void;
  onResume: () => void;
  onClose: () => void;
}

const NO_DESTS: Dests = new Map();
// tailles de table de hachage suggerees (Mo) -- la saisie reste libre
const HASH_SIZES_MB = [64, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768];
const MIN_HASH_MB = 16;

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
function renderPv(
  pv: PvMove[],
  analyzedFen: string | null,
  onHover: (move: PvMove, e: React.MouseEvent) => void,
  onPlayPvLine: (moves: PvMove[], uptoIndex: number) => void
) {
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
      // Rejoue toute la ligne jusqu'a ce coup (voir handlePlayPvLine,
      // GameTab.tsx) : un clic plus loin dans la variante avance donc
      // directement jusque-la, pas seulement d'un coup.
      <span className="engine-pv-move" key={i} onMouseEnter={(e) => onHover(move, e)} onClick={() => onPlayPvLine(pv, i)}>
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
  hashMb,
  onHashChange,
  maxHashMb,
  lines,
  error,
  analyzedFen,
  boardTheme,
  orientation,
  onPlayPvLine,
  onStart,
  onStop,
  onPause,
  onResume,
  onClose,
}: EnginePanelProps) {
  const { t, i18n } = useTranslation();
  const [showSettings, setShowSettings] = useState(() => !enginePath);
  // Saisie libre de la memoire : validee a la sortie du champ ou sur Entree
  // seulement, pas a chaque chiffre tape -- chaque changement de taille fait
  // realouer et vider la table de hachage du moteur.
  const [hashInput, setHashInput] = useState(String(hashMb));
  useEffect(() => setHashInput(String(hashMb)), [hashMb]);
  function commitHash() {
    const typed = Math.floor(Number(hashInput));
    if (!Number.isFinite(typed) || typed <= 0) {
      setHashInput(String(hashMb));
      return;
    }
    const clamped = Math.min(Math.max(typed, MIN_HASH_MB), maxHashMb ?? typed);
    setHashInput(String(clamped));
    if (clamped !== hashMb) onHashChange(clamped);
  }
  // Ligne d'etat : statistiques de recherche de la meilleure ligne (les
  // memes pour toutes les lignes d'une meme iteration).
  const statusLine = lines.find((l) => l.multipv === 1) ?? lines[0] ?? null;
  const compact = new Intl.NumberFormat(i18n.language, { notation: "compact", maximumFractionDigits: 1 });
  const statusParts: string[] = [];
  if (statusLine) {
    statusParts.push(
      t("engine.statusDepth", { depth: statusLine.seldepth !== null ? `${statusLine.depth}/${statusLine.seldepth}` : String(statusLine.depth) })
    );
    if (statusLine.nps !== null) statusParts.push(t("engine.statusNps", { value: compact.format(statusLine.nps) }));
    if (statusLine.nodes !== null) statusParts.push(t("engine.statusNodes", { value: compact.format(statusLine.nodes) }));
    if (statusLine.hashfull !== null) statusParts.push(t("engine.statusHash", { value: Math.round(statusLine.hashfull / 10) }));
  }
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
              <input
                type="number"
                min={1}
                value={depth}
                onChange={(e) => onDepthChange(Math.max(1, Number(e.currentTarget.value) || 1))}
                title={t("engine.depthUnbounded")}
              />
            </label>
            <label className="engine-field">
              {t("engine.multiPv")}
              <select value={multiPv} onChange={(e) => onMultiPvChange(Number(e.currentTarget.value))}>
                <option value={1}>1</option>
                <option value={2}>2</option>
                <option value={3}>3</option>
                <option value={4}>4</option>
                <option value={5}>5</option>
                <option value={6}>6</option>
                <option value={7}>7</option>
                <option value={8}>8</option>
                <option value={9}>9</option>
                <option value={10}>10</option>
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
            <label className="engine-field" title={t("engine.hashHint")}>
              {maxHashMb !== null ? t("engine.hashMax", { max: maxHashMb }) : t("engine.hash")}
              <div className="engine-hash-row">
                <input
                  type="number"
                  className="engine-hash-input"
                  min={MIN_HASH_MB}
                  max={maxHashMb ?? undefined}
                  step={64}
                  value={hashInput}
                  onChange={(e) => setHashInput(e.currentTarget.value)}
                  onBlur={commitHash}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitHash();
                  }}
                />
                {/* Tailles courantes dans une vraie liste deroulante : la
                    <datalist> d'un champ numerique ne s'ouvre pas dans la
                    WebView de Windows (rapporte par l'utilisateur). Toujours
                    sur l'entree vide : sert de raccourci, la valeur en cours
                    se lit dans le champ. */}
                <select
                  className="engine-hash-presets"
                  value=""
                  aria-label={t("engine.hashPresets")}
                  title={t("engine.hashPresets")}
                  onChange={(e) => {
                    const size = Number(e.currentTarget.value);
                    if (size > 0 && size !== hashMb) onHashChange(size);
                  }}
                >
                  <option value="" disabled hidden>
                    …
                  </option>
                  {HASH_SIZES_MB.filter((size) => maxHashMb === null || size <= maxHashMb).map((size) => (
                    <option key={size} value={size}>
                      {size}
                    </option>
                  ))}
                </select>
              </div>
            </label>
          </div>
        </div>
      )}

      {error && <p className="error">{error}</p>}

      {running && (
        <div className="engine-lines">
          {paused && <p className="engine-line engine-line--paused">{t("engine.paused")}</p>}
          {lines.length === 0 && !paused && <p className="engine-line">{t("engine.analyzing")}</p>}
          {statusParts.length > 0 && (
            <p
              className={"engine-status" + (statusLine?.hashfull !== null && statusLine?.hashfull !== undefined && statusLine.hashfull >= 900 ? " engine-status--hash-full" : "")}
              title={t("engine.statusHint")}
            >
              {statusParts.join(" · ")}
            </p>
          )}
          {lines.map((info) => (
            <div key={info.multipv} className="engine-result">
              <span className="engine-score">{formatScore(info)}</span>
              {info.wdl && (
                <span
                  className="engine-wdl"
                  title={t("engine.wdlTitle", {
                    white: Math.round(info.wdl[0] / 10),
                    draw: Math.round(info.wdl[1] / 10),
                    black: Math.round(info.wdl[2] / 10),
                  })}
                >
                  <span className="engine-wdl-white" style={{ width: `${info.wdl[0] / 10}%` }} />
                  <span className="engine-wdl-draw" style={{ width: `${info.wdl[1] / 10}%` }} />
                  <span className="engine-wdl-black" style={{ width: `${info.wdl[2] / 10}%` }} />
                </span>
              )}
              <span className="engine-line" onMouseLeave={() => setHover(null)}>
                {renderPv(info.pv, analyzedFen, handleMoveHover, onPlayPvLine)}
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
