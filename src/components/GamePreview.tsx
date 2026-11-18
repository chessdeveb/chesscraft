import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";
import { loadStoredBoardTheme } from "../boardTheme";
import { isEditableTarget } from "../annotationShortcuts";
import type { NodeDto, TreeDto } from "../types";
import { Board } from "./Board";

const NO_DESTS: Dests = new Map();
const NO_OP = () => {};
/** Un coup par seconde : assez lent pour suivre la partie, assez vif pour
    qu'un coup d'oeil donne l'idee de la partie. */
const STEP_MS = 1000;
const POPUP_WIDTH = 320;
const POPUP_HEIGHT = 490;
const EDGE_MARGIN = 8;

export interface GamePreviewInfo {
  white: string;
  black: string;
  date: string;
  result: string;
}

interface GamePreviewProps {
  basePath: string;
  gameId: number;
  info: GamePreviewInfo;
  /** Point clique (coordonnees fenetre) : l'apercu s'ouvre a cote. */
  anchor: { x: number; y: number };
  onOpenGame: () => void;
  onClose: () => void;
}

/** Les noeuds de la ligne principale, de la position de depart au dernier coup. */
function mainline(tree: TreeDto): NodeDto[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const out: NodeDto[] = [];
  let node = byId.get(tree.root);
  while (node) {
    out.push(node);
    node = node.children.length > 0 ? byId.get(node.children[0]) : undefined;
  }
  return out;
}

/** "12. Nf3" / "12... Nc6" : numero et trait lus dans la position qui precede
    le coup (voir Notation.tsx), donc justes aussi pour une partie qui
    demarre d'une position. */
function moveLabel(line: NodeDto[], index: number): string {
  const node = line[index];
  if (index === 0 || !node.san) return "";
  const before = line[index - 1].fen.split(" ");
  const number = Number(before[5]) || Math.ceil(index / 2);
  return `${number}${before[1] === "b" ? "..." : "."} ${node.san}`;
}

/** Apercu d'une partie de la liste (Alt+clic dans l'onglet Parties) : petit
    echiquier qui rejoue la partie tout seul, avec reprise a la main. Reste
    affiche jusqu'a Echap, un clic ailleurs ou un autre Alt+clic. Porte dans
    <body> : dockview pose transform/contain sur ses panneaux, ce qui piegerait
    un element en position fixe dans les limites du panneau (voir
    NagClusterButton.tsx). */
export function GamePreview({ basePath, gameId, info, anchor, onOpenGame, onClose }: GamePreviewProps) {
  const { t } = useTranslation();
  const [tree, setTree] = useState<TreeDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(true);
  const [flipped, setFlipped] = useState(false);
  const popupRef = useRef<HTMLDivElement | null>(null);
  const boardTheme = useMemo(() => loadStoredBoardTheme(), []);

  useEffect(() => {
    let cancelled = false;
    setTree(null);
    setError(null);
    setIndex(0);
    setPlaying(true);
    invoke<TreeDto>("preview_game", { path: basePath, id: gameId })
      .then((loaded) => {
        if (!cancelled) setTree(loaded);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [basePath, gameId]);

  const line = useMemo(() => (tree ? mainline(tree) : []), [tree]);
  const last = line.length - 1;

  // deroulement automatique : un coup a intervalle regulier, s'arrete a la fin
  useEffect(() => {
    if (!playing || line.length === 0) return;
    if (index >= last) {
      setPlaying(false);
      return;
    }
    const timer = setTimeout(() => setIndex((i) => Math.min(i + 1, last)), STEP_MS);
    return () => clearTimeout(timer);
  }, [playing, index, last, line.length]);

  // navigation a la main : met le deroulement automatique en pause
  const goTo = useCallback(
    (target: number) => {
      setPlaying(false);
      setIndex(Math.max(0, Math.min(target, last)));
    },
    [last]
  );
  const togglePlay = useCallback(() => {
    if (line.length === 0) return;
    if (!playing && index >= last) {
      // fin de partie : relancer rejoue depuis le debut
      setIndex(0);
      setPlaying(true);
    } else {
      setPlaying((p) => !p);
    }
  }, [playing, index, last, line.length]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (isEditableTarget(e.target)) return;
      let handled = true;
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowLeft") goTo(index - 1);
      else if (e.key === "ArrowRight") goTo(index + 1);
      else if (e.key === "Home") goTo(0);
      else if (e.key === "End") goTo(last);
      else if (e.key === " " && !e.ctrlKey && !e.altKey && !e.metaKey) togglePlay();
      else handled = false;
      if (handled) {
        e.preventDefault();
        e.stopPropagation();
      }
    }
    function onMouseDown(e: MouseEvent) {
      if (popupRef.current && !popupRef.current.contains(e.target as Node)) onClose();
    }
    // phase de capture : passe avant les raccourcis de la fenetre
    window.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("mousedown", onMouseDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("mousedown", onMouseDown, true);
    };
  }, [goTo, togglePlay, index, last, onClose]);

  const left = Math.max(EDGE_MARGIN, Math.min(anchor.x + 16, window.innerWidth - POPUP_WIDTH - EDGE_MARGIN));
  const top = Math.max(EDGE_MARGIN, Math.min(anchor.y + 12, window.innerHeight - POPUP_HEIGHT - EDGE_MARGIN));
  const node = line[index];
  const title = `${info.white} – ${info.black}`;

  return createPortal(
    <div
      ref={popupRef}
      className="game-preview"
      style={{ left, top, width: POPUP_WIDTH }}
      role="dialog"
      aria-label={t("gamePreview.title", { game: title })}
      onWheel={(e) => goTo(index + (e.deltaY > 0 ? 1 : -1))}
    >
      <div className="game-preview-header">
        <div className="game-preview-names" title={title}>
          {title}
        </div>
        <button type="button" className="game-preview-close" onClick={onClose} title={t("gamePreview.close")} aria-label={t("gamePreview.close")}>
          ✕
        </button>
      </div>
      <div className="game-preview-meta">{[info.date, info.result].filter(Boolean).join(" · ")}</div>

      {error && <p className="error">{error}</p>}
      {!node && !error && <p className="notation-empty">{t("common.loading")}</p>}
      {node && (
        <>
          <div className="game-preview-board">
            <Board
              fen={node.fen}
              turnColor={node.turn as Color}
              dests={NO_DESTS}
              shapes={[]}
              theme={boardTheme}
              lastMove={node.from && node.to ? [node.from as Key, node.to as Key] : null}
              orientation={flipped ? "black" : "white"}
              onMove={NO_OP}
              onShapesChange={NO_OP}
            />
          </div>
          <div className="game-preview-move">{moveLabel(line, index) || t("gamePreview.startPosition")}</div>
          <div className="game-preview-controls">
            <button type="button" onClick={() => goTo(0)} title={t("gamePreview.start")} aria-label={t("gamePreview.start")}>
              ⏮
            </button>
            <button type="button" onClick={() => goTo(index - 1)} title={t("gamePreview.prev")} aria-label={t("gamePreview.prev")}>
              ◀
            </button>
            <button
              type="button"
              className="game-preview-play"
              onClick={togglePlay}
              title={playing ? t("gamePreview.pause") : t("gamePreview.play")}
              aria-label={playing ? t("gamePreview.pause") : t("gamePreview.play")}
            >
              {playing ? "⏸" : "▶"}
            </button>
            <button type="button" onClick={() => goTo(index + 1)} title={t("gamePreview.next")} aria-label={t("gamePreview.next")}>
              ▶|
            </button>
            <button type="button" onClick={() => goTo(last)} title={t("gamePreview.end")} aria-label={t("gamePreview.end")}>
              ⏭
            </button>
            <button type="button" onClick={() => setFlipped((f) => !f)} title={t("gamePreview.flip")} aria-label={t("gamePreview.flip")}>
              🔄
            </button>
          </div>
          <div className="game-preview-footer">
            <button type="button" onClick={onOpenGame}>
              {t("gamePreview.open")}
            </button>
          </div>
        </>
      )}
    </div>,
    document.body
  );
}
