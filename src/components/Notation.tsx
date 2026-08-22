import { useEffect, useState, type ReactNode } from "react";
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
        placeholder="Commentaire..."
      />
      <button onClick={onClose} title="Fermer" aria-label="Fermer l'éditeur de commentaire">
        ✕
      </button>
    </div>
  );
}

export function Notation({ tree, currentNodeId, onSelect, onSetComment, onToggleNag }: NotationProps) {
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

  return (
    <div className="notation-wrapper">
      <div className="notation">
        {firstMoveId !== undefined ? (
          renderLine(firstMoveId, true)
        ) : (
          <p className="notation-empty">Aucun coup joué</p>
        )}
      </div>
      {currentNode && (
        <div className="annotate-bar">
          <div className="annotate-toolbar">
            {MOVE_NAG_CHOICES.map((n) => (
              <button
                key={n.code}
                title={n.label}
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
                title={n.label}
                className={"nag-button" + (activeEvalNag === n.code ? " nag-button--active" : "")}
                onClick={() => onToggleNag(n.code)}
              >
                {n.symbol}
              </button>
            ))}
            <span className="annotate-toolbar-sep" />
            <button
              className={"annotate-icon" + (editing === "before" ? " annotate-icon--active" : "")}
              title="Commentaire avant le coup"
              aria-label="Commentaire avant le coup"
              onClick={() => setEditing(editing === "before" ? null : "before")}
            >
              💬 avant
            </button>
            <button
              className={"annotate-icon" + (editing === "after" ? " annotate-icon--active" : "")}
              title="Commentaire après le coup"
              aria-label="Commentaire après le coup"
              onClick={() => setEditing(editing === "after" ? null : "after")}
            >
              💬 après
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
