import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";
import { Board } from "./Board";
import { nagSymbol } from "../nag";
import type { PvMove } from "../types";
import type { TrainingFeedback, TrainingSessionSummary } from "../AppContext";

interface TrainingPanelProps {
  fen: string | null;
  side: "white" | "black" | null;
  away: boolean;
  feedback: TrainingFeedback | null;
  busy: boolean;
  error: string | null;
  atEnd: boolean;
  accuracy: number | null;
  summary: TrainingSessionSummary | null;
  depth: number;
  onDepthChange: (depth: number) => void;
  threads: number;
  onThreadsChange: (threads: number) => void;
  maxThreads: number;
  boardTheme: string;
  currentNodeId: number;
  onStart: (nodeId: number) => void;
  onStop: () => void;
  onResume: () => void;
  onContinue: () => void;
}

const NO_DESTS: Dests = new Map();

interface HoverPreview {
  fen: string;
  from: string;
  to: string;
  x: number;
  y: number;
}

// Meme principe que fenMoveContext/renderPv (EnginePanel.tsx) : pas
// mutualise (l'apercu au survol d'EnginePanel n'est pas exporte), duplication
// jugee moins couteuse qu'extraire un utilitaire partage pour un seul
// appelant de plus.
function fenMoveContext(fen: string): { whiteToMove: boolean; moveNumber: number } {
  const parts = fen.split(" ");
  return { whiteToMove: parts[1] !== "b", moveNumber: Number(parts[5]) || 1 };
}

function renderRefutation(moves: PvMove[], fenBefore: string, onHover: (move: PvMove, e: React.MouseEvent) => void) {
  const { whiteToMove, moveNumber } = fenMoveContext(fenBefore);
  let number = moveNumber;
  let isWhite = whiteToMove;
  const out: React.ReactNode[] = [];
  moves.forEach((move, i) => {
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

export function TrainingPanel({
  fen,
  side,
  away,
  feedback,
  busy,
  error,
  atEnd,
  accuracy,
  summary,
  depth,
  onDepthChange,
  threads,
  onThreadsChange,
  maxThreads,
  boardTheme,
  currentNodeId,
  onStart,
  onStop,
  onResume,
  onContinue,
}: TrainingPanelProps) {
  const { t } = useTranslation();
  const [hover, setHover] = useState<HoverPreview | null>(null);

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

  // Reglage moteur propre a l'entrainement (voir GameTab.tsx pour pourquoi
  // ca ne doit pas etre celui, invisible ici, du rapport de partie) --
  // affiche sur l'ecran de depart ET pendant la session, pour rester
  // ajustable sans devoir arreter/relancer l'exercice.
  const settingsRow = (
    <div className="engine-settings-row">
      <label className="engine-field">
        {t("engine.depth")}
        <select value={depth} onChange={(e) => onDepthChange(Number(e.currentTarget.value))}>
          <option value={6}>6</option>
          <option value={8}>8</option>
          <option value={10}>10</option>
          <option value={14}>14</option>
          <option value={18}>18</option>
        </select>
      </label>
      <label className="engine-field">
        {t("engine.threads")}
        <select value={threads} onChange={(e) => onThreadsChange(Number(e.currentTarget.value))}>
          {Array.from({ length: Math.max(1, maxThreads) }, (_, i) => i + 1).map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </label>
    </div>
  );

  if (!fen || !side) {
    return (
      <div className="dock-panel-content training-panel training-panel--start">
        <p>{t("training.intro")}</p>
        {settingsRow}
        <button onClick={() => onStart(currentNodeId)}>{t("training.start")}</button>
      </div>
    );
  }

  if (summary) {
    return (
      <div className="dock-panel-content training-panel training-panel--start">
        <h3>{t("training.summaryTitle")}</h3>
        <div className="game-report-side">
          <div className="game-report-side-header">
            <span className="game-report-side-accuracy">{t("gameReport.accuracy", { value: summary.accuracy.toFixed(1) })}</span>
          </div>
          <div className="game-report-side-counts">
            <span className="game-report-count game-report-count--inaccuracy">{summary.inaccuracies} {t("gameReport.inaccuracies")}</span>
            <span className="game-report-count game-report-count--mistake">{summary.mistakes} {t("gameReport.mistakes")}</span>
            <span className="game-report-count game-report-count--blunder">{summary.blunders} {t("gameReport.blunders")}</span>
          </div>
        </div>
        <p className="training-summary-note">{t("training.summaryNote", { count: summary.rounds })}</p>
        <button onClick={onStop}>{t("training.newSession")}</button>
      </div>
    );
  }

  return (
    <div className="dock-panel-content training-panel">
      {/* Pas d'echiquier ici : l'exercice se joue sur l'echiquier principal
          (voir boardDests/boardMove, GameTab.tsx), ce panneau n'affiche que
          les informations de la session. */}
      <div className="training-panel-side">
        {settingsRow}
        {accuracy !== null && (
          <p className="training-accuracy">{t("gameReport.accuracy", { value: accuracy.toFixed(1) })}</p>
        )}
        <div className="training-panel-feedback">
          {error && <p className="error">{error}</p>}
          {busy && <p className="status-message">{t("training.analyzing")}</p>}
          {!busy && !feedback && atEnd && <p className="notation-empty">{t("training.lineEnd")}</p>}
          {away && (
            <p className="training-away">
              {t("training.away")} <button onClick={onResume}>{t("training.resume")}</button>
            </p>
          )}
          {!busy && !feedback && !atEnd && !away && (
            <p className="status-message">{t(side === "white" ? "training.yourMoveWhite" : "training.yourMoveBlack")}</p>
          )}
          {feedback && (
            <div className="training-feedback-card">
              {feedback.matchesGame ? (
                <p className="training-feedback-ok">✓ {t("training.correct")}</p>
              ) : feedback.classification !== null ? (
                <p className="training-feedback-bad">
                  {nagSymbol(feedback.classification)} {t(`nagAll.${feedback.classification}`)} —{" "}
                  {t("training.gamePlayedInstead", { san: feedback.gameSan })}
                </p>
              ) : feedback.gameMoveComparison === "better" ? (
                <p className="training-feedback-better">★ {t("training.betterThanGame", { san: feedback.gameSan })}</p>
              ) : feedback.gameMoveComparison === "worse" ? (
                <p className="training-feedback-bad">{t("training.worseThanGame", { san: feedback.gameSan })}</p>
              ) : (
                <p className="training-feedback-neutral">{t("training.similarToGame", { san: feedback.gameSan })}</p>
              )}
              {/* Independant du coup essaye : le coup de la partie lui-meme
                  n'etait pas forcement optimal, meme quand on vient de le
                  rejouer a l'identique (voir handleTrainingMove). */}
              {feedback.betterGameMoveSan && (
                <p className="training-feedback-neutral">
                  {feedback.gameMoveClassification !== null && `${nagSymbol(feedback.gameMoveClassification)} `}
                  {t("training.gameMoveBetterAlternative", { san: feedback.betterGameMoveSan })}
                </p>
              )}
              {feedback.refutation.length > 0 && (
                <p className="training-refutation" onMouseLeave={() => setHover(null)}>
                  {t("training.refutation")} {renderRefutation(feedback.refutation, fen, handleMoveHover)}
                </p>
              )}
              {(!feedback.matchesGame || feedback.betterGameMoveSan) && <button onClick={onContinue}>{t("training.continue")}</button>}
            </div>
          )}
        </div>
        <button className="training-stop" onClick={onStop}>
          {t("training.stop")}
        </button>
      </div>
      {hover &&
        createPortal(
          // Portail direct dans <body>, meme raison que EnginePanel : dockview
          // pose transform/contain sur ses conteneurs de panneaux, ce qui
          // emprisonnerait un apercu position:fixed non porte dans les limites
          // du panneau Entrainement.
          <div className="engine-hover-preview" style={{ left: hover.x, top: hover.y }}>
            <Board
              fen={hover.fen}
              turnColor={(hover.fen.split(" ")[1] === "b" ? "black" : "white") as Color}
              dests={NO_DESTS}
              shapes={[]}
              theme={boardTheme}
              lastMove={hover.from && hover.to ? [hover.from as Key, hover.to as Key] : null}
              orientation={side as Color}
              onMove={() => {}}
              onShapesChange={() => {}}
            />
          </div>,
          document.body
        )}
    </div>
  );
}
