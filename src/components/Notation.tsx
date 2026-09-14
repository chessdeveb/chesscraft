import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { EVAL_NAG_CHOICES, EVAL_NAG_CODES, MOVE_NAG_CHOICES, MOVE_NAG_CODES, nagSymbol } from "../nag";
import type { GameReportProgress, GameReportSummary, NodeDto, TreeDto } from "../types";
import { GameReportChart } from "./GameReportChart";
import { GameReportDialog } from "./GameReportDialog";

interface NotationProps {
  tree: TreeDto;
  currentNodeId: number;
  onSelect: (id: number) => void;
  onSetComment?: (before: string, after: string) => void;
  onToggleNag?: (code: number) => void;
  /** Fait de la branche du coup courant la ligne principale (bouton a
      gauche des NAG). */
  onPromoteVariation?: () => void;
  /** Supprime la branche du coup courant (bouton a gauche des NAG). */
  onDeleteVariation?: () => void;
  /** Aperçu en lecture seule (Explorateur) : pas de barre d'annotation. */
  readOnly?: boolean;
  /** Nombre de coeurs logiques disponibles, pour le reglage du rapport de
      partie -- absent en lecture seule (le bouton n'est alors pas affiche). */
  maxThreads?: number;
  /** Depth/threads choisis dans la pop-up de reglage -- lance l'analyse cote backend. */
  onStartGameReport?: (depth: number, threads: number) => void;
  gameReportRunning?: boolean;
  gameReportProgress?: GameReportProgress | null;
  gameReportError?: string | null;
  gameReportSummary?: GameReportSummary | null;
}

const CLOCK_RE = /\[%clk\s+([\d:.]+)\]/;

/** Extrait l'annotation d'horloge "[%clk H:MM:SS]" d'un commentaire PGN
    (convention lichess/chess.com, coup joue par coup) -- renvoie l'heure
    telle quelle pour l'affichage, et le reste du commentaire debarrasse de
    cette annotation (jamais affichee telle quelle, illisible en brut). */
function parseClock(comment: string | null): { clock: string | null; rest: string } {
  if (!comment) return { clock: null, rest: "" };
  const match = comment.match(CLOCK_RE);
  return { clock: match ? match[1] : null, rest: comment.replace(CLOCK_RE, "").trim() };
}

function InlineCommentEditor({
  value,
  onSave,
  onClose,
}: {
  value: string;
  onSave: (text: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [text, setText] = useState(value);

  useEffect(() => {
    const timeout = setTimeout(() => onSave(text), 500);
    return () => clearTimeout(timeout);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text]);

  return (
    <div className="comment-editor-inline">
      <textarea
        autoFocus
        rows={2}
        value={text}
        onChange={(e) => setText(e.currentTarget.value)}
        placeholder={t("notation.commentPlaceholder")}
      />
      <button onClick={onClose} title={t("notation.close")} aria-label={t("notation.closeCommentEditor")}>
        ✕
      </button>
    </div>
  );
}

export function Notation({
  tree,
  currentNodeId,
  onSelect,
  onSetComment,
  onToggleNag,
  onPromoteVariation,
  onDeleteVariation,
  readOnly,
  maxThreads,
  onStartGameReport,
  gameReportRunning,
  gameReportProgress,
  gameReportError,
  gameReportSummary,
}: NotationProps) {
  const { t } = useTranslation();
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const [editing, setEditing] = useState<"before" | "after" | null>(null);
  const [showReportDialog, setShowReportDialog] = useState(false);
  const currentMoveRef = useRef<HTMLSpanElement | null>(null);

  // navigation clavier (fleches) ou via l'echiquier : le coup courant doit
  // rester visible sans que l'utilisateur ait a scroller lui-meme dans une
  // partie longue.
  useEffect(() => {
    currentMoveRef.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [currentNodeId]);

  function selectAndReset(id: number) {
    if (id !== currentNodeId) {
      setEditing(null);
    }
    onSelect(id);
  }

  function depthOf(id: number): number {
    let depth = 0;
    let node = byId.get(id);
    while (node && node.parent !== null) {
      depth += 1;
      node = byId.get(node.parent);
    }
    return depth;
  }

  function renderMove(node: NodeDto, showEllipsis: boolean): ReactNode {
    const depth = depthOf(node.id);
    const isWhite = depth % 2 === 1;
    const moveNumber = Math.ceil(depth / 2);
    const label = isWhite ? `${moveNumber}.` : showEllipsis ? `${moveNumber}...` : "";
    const nagText = node.nags.map(nagSymbol).join("");
    const isCurrent = node.id === currentNodeId;
    const before = parseClock(node.commentBefore);
    const after = parseClock(node.commentAfter);

    return (
      <span key={node.id}>
        {label && <span className="move-number">{label} </span>}
        {before.rest && <span className="comment">{before.rest} </span>}
        {before.clock && <span className="move-clock">🕐{before.clock} </span>}
        <span
          ref={isCurrent ? currentMoveRef : undefined}
          className={"move" + (isCurrent ? " move--current" : "")}
          onClick={() => selectAndReset(node.id)}
        >
          {node.san}
          {nagText}
        </span>{" "}
        {after.clock && <span className="move-clock">🕐{after.clock} </span>}
        {after.rest && <span className="comment">{after.rest} </span>}
      </span>
    );
  }

  function renderLine(startId: number, forceNumberOnFirst: boolean): ReactNode[] {
    const out: ReactNode[] = [];
    let node = byId.get(startId);
    let showEllipsis = forceNumberOnFirst;

    while (node) {
      out.push(renderMove(node, showEllipsis));

      const [mainChildId, ...variationIds] = node.children;
      for (const varId of variationIds) {
        out.push(
          <span key={`var-${varId}`} className="variation">
            ({renderLine(varId, true)})
          </span>
        );
        out.push(" ");
      }

      if (mainChildId === undefined) break;
      node = byId.get(mainChildId);
      showEllipsis = variationIds.length > 0;
    }
    return out;
  }

  const rootNode = byId.get(tree.root);
  const firstMoveId = rootNode?.children[0];
  const currentNode = byId.get(currentNodeId);
  const activeMoveNag = currentNode?.nags.find((n) => MOVE_NAG_CODES.has(n));
  const activeEvalNag = currentNode?.nags.find((n) => EVAL_NAG_CODES.has(n));

  // Un coup est "sur la ligne principale" si, a chaque embranchement
  // remonte jusqu'a la racine, il est le premier enfant de son parent --
  // desactive alors le bouton "promouvoir" (rien a promouvoir).
  function isOnMainline(id: number): boolean {
    let node = byId.get(id);
    while (node && node.parent !== null) {
      const parent = byId.get(node.parent);
      if (!parent || parent.children[0] !== node.id) return false;
      node = parent;
    }
    return true;
  }
  const currentIsVariation = !!currentNode && currentNode.parent !== null && !isOnMainline(currentNode.id);

  const elo = (color: "White" | "Black") => {
    const value = tree.tags[`${color}Elo`];
    return value && value !== "?" ? ` (${value})` : "";
  };
  const white = tree.tags.White ?? "?";
  const black = tree.tags.Black ?? "?";
  const event = tree.tags.Event;
  const date = tree.tags.Date;
  const result = tree.tags.Result;

  return (
    <div className="notation-wrapper">
      {!readOnly && (
        <div className="notation-toolbar">
          {gameReportError && <span className="error">{gameReportError}</span>}
          {gameReportRunning && gameReportProgress && (
            <span className="notation-toolbar-progress">
              {gameReportProgress.total > 0
                ? t("gameReport.progress", { done: gameReportProgress.done, total: gameReportProgress.total })
                : t("gameReport.starting")}
            </span>
          )}
          <button disabled={gameReportRunning} onClick={() => setShowReportDialog(true)}>
            {t("gameReport.button")}
          </button>
        </div>
      )}
      {(white !== "?" || black !== "?" || event || date) && (
        <div className="notation-header">
          <div className="notation-header-players">
            {white}
            {elo("White")} – {black}
            {elo("Black")}
            {result && <span className="notation-header-result">{result}</span>}
          </div>
          {(event || date) && (
            <div className="notation-header-meta">
              {[event, date].filter(Boolean).join(", ")}
            </div>
          )}
        </div>
      )}
      <div className="notation">
        {firstMoveId !== undefined ? (
          renderLine(firstMoveId, true)
        ) : (
          <p className="notation-empty">{t("notation.empty")}</p>
        )}
        {tree.tags.Result && <span className="notation-result">{tree.tags.Result}</span>}
      </div>
      {currentNode && !readOnly && (
        <div className="annotate-bar">
          <div className="annotate-toolbar">
            <button
              className="annotate-icon"
              title={t("notation.promoteVariation")}
              aria-label={t("notation.promoteVariation")}
              disabled={!currentIsVariation}
              onClick={() => onPromoteVariation?.()}
            >
              ⬆️
            </button>
            <button
              className="annotate-icon"
              title={t("notation.deleteVariation")}
              aria-label={t("notation.deleteVariation")}
              disabled={!currentNode || currentNode.parent === null}
              onClick={() => onDeleteVariation?.()}
            >
              🗑️
            </button>
            <span className="annotate-toolbar-sep" />
            {MOVE_NAG_CHOICES.map((n) => (
              <button
                key={n.code}
                title={t(n.labelKey)}
                className={"nag-button" + (activeMoveNag === n.code ? " nag-button--active" : "")}
                onClick={() => onToggleNag?.(n.code)}
              >
                {n.symbol}
              </button>
            ))}
            <span className="annotate-toolbar-sep" />
            {EVAL_NAG_CHOICES.map((n) => (
              <button
                key={n.code}
                title={t(n.labelKey)}
                className={"nag-button" + (activeEvalNag === n.code ? " nag-button--active" : "")}
                onClick={() => onToggleNag?.(n.code)}
              >
                {n.symbol}
              </button>
            ))}
            <span className="annotate-toolbar-sep" />
            <button
              className={"annotate-icon" + (editing === "before" ? " annotate-icon--active" : "")}
              title={t("notation.commentBefore")}
              aria-label={t("notation.commentBefore")}
              onClick={() => setEditing(editing === "before" ? null : "before")}
            >
              {t("notation.commentBeforeShort")}
            </button>
            <button
              className={"annotate-icon" + (editing === "after" ? " annotate-icon--active" : "")}
              title={t("notation.commentAfter")}
              aria-label={t("notation.commentAfter")}
              onClick={() => setEditing(editing === "after" ? null : "after")}
            >
              {t("notation.commentAfterShort")}
            </button>
          </div>
          {editing === "before" && (
            <InlineCommentEditor
              value={currentNode.commentBefore ?? ""}
              onSave={(text) => onSetComment?.(text, currentNode.commentAfter ?? "")}
              onClose={() => setEditing(null)}
            />
          )}
          {editing === "after" && (
            <InlineCommentEditor
              value={currentNode.commentAfter ?? ""}
              onSave={(text) => onSetComment?.(currentNode.commentBefore ?? "", text)}
              onClose={() => setEditing(null)}
            />
          )}
        </div>
      )}
      {!readOnly && gameReportSummary && (
        <div className="game-report">
          <GameReportChart moves={gameReportSummary.moves} currentNodeId={currentNodeId} onSelect={selectAndReset} />
          <div className="game-report-stats">
            {(
              [
                ["white", white !== "?" ? white : t("common.white"), gameReportSummary.white],
                ["black", black !== "?" ? black : t("common.black"), gameReportSummary.black],
              ] as const
            ).map(([side, name, stats]) => (
              <div className="game-report-side" key={side}>
                <div className="game-report-side-header">
                  <span className={`game-report-dot game-report-dot--${side}`} />
                  <span className="game-report-side-name">{name}</span>
                  <span className="game-report-side-accuracy">{t("gameReport.accuracy", { value: stats.accuracy.toFixed(1) })}</span>
                </div>
                <div className="game-report-side-counts">
                  <span className="game-report-count game-report-count--inaccuracy">
                    {stats.inaccuracies} {t("gameReport.inaccuracies")}
                  </span>
                  <span className="game-report-count game-report-count--mistake">
                    {stats.mistakes} {t("gameReport.mistakes")}
                  </span>
                  <span className="game-report-count game-report-count--blunder">
                    {stats.blunders} {t("gameReport.blunders")}
                  </span>
                  <span className="game-report-count game-report-count--acpl">
                    {t("gameReport.acpl", { value: Math.round(stats.avgCentipawnLoss) })}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
      {showReportDialog && (
        <GameReportDialog
          maxThreads={maxThreads ?? 1}
          onCancel={() => setShowReportDialog(false)}
          onConfirm={(depth, threads) => {
            setShowReportDialog(false);
            onStartGameReport?.(depth, threads);
          }}
        />
      )}
    </div>
  );
}
