import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { EVAL_NAG_CHOICES, EVAL_NAG_CODES, MOVE_NAG_CHOICES, MOVE_NAG_CODES, nagSymbol } from "../nag";
import type { NodeDto, TreeDto } from "../types";

interface NotationProps {
  tree: TreeDto;
  currentNodeId: number;
  onSelect: (id: number) => void;
  onSetComment: (before: string, after: string) => void;
  onToggleNag: (code: number) => void;
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

export function Notation({ tree, currentNodeId, onSelect, onSetComment, onToggleNag }: NotationProps) {
  const { t } = useTranslation();
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const [editing, setEditing] = useState<"before" | "after" | null>(null);

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

    return (
      <span key={node.id}>
        {label && <span className="move-number">{label} </span>}
        {node.commentBefore && <span className="comment">{node.commentBefore} </span>}
        <span className={"move" + (isCurrent ? " move--current" : "")} onClick={() => selectAndReset(node.id)}>
          {node.san}
          {nagText}
        </span>{" "}
        {node.commentAfter && <span className="comment">{node.commentAfter} </span>}
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
      {currentNode && (
        <div className="annotate-bar">
          <div className="annotate-toolbar">
            {MOVE_NAG_CHOICES.map((n) => (
              <button
                key={n.code}
                title={t(n.labelKey)}
                className={"nag-button" + (activeMoveNag === n.code ? " nag-button--active" : "")}
                onClick={() => onToggleNag(n.code)}
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
                onClick={() => onToggleNag(n.code)}
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
              onSave={(text) => onSetComment(text, currentNode.commentAfter ?? "")}
              onClose={() => setEditing(null)}
            />
          )}
          {editing === "after" && (
            <InlineCommentEditor
              value={currentNode.commentAfter ?? ""}
              onSave={(text) => onSetComment(currentNode.commentBefore ?? "", text)}
              onClose={() => setEditing(null)}
            />
          )}
        </div>
      )}
    </div>
  );
}
