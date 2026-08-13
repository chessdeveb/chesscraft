import { useEffect, useRef, useState } from "react";
import { NAG_CHOICES } from "../nag";
import type { NodeDto } from "../types";

interface AnnotationPanelProps {
  node: NodeDto;
  onSetComment: (before: string, after: string) => void;
  onToggleNag: (code: number) => void;
}

export function AnnotationPanel({ node, onSetComment, onToggleNag }: AnnotationPanelProps) {
  const [before, setBefore] = useState(node.commentBefore ?? "");
  const [after, setAfter] = useState(node.commentAfter ?? "");
  const skipNextSaveRef = useRef(true);

  // changement de noeud : on recharge les commentaires sans relancer une sauvegarde
  useEffect(() => {
    skipNextSaveRef.current = true;
    setBefore(node.commentBefore ?? "");
    setAfter(node.commentAfter ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [node.id]);

  // sauvegarde automatique une fois la frappe stabilisee
  useEffect(() => {
    if (skipNextSaveRef.current) {
      skipNextSaveRef.current = false;
      return;
    }
    const timeout = setTimeout(() => onSetComment(before, after), 500);
    return () => clearTimeout(timeout);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [before, after]);

  const activeNag = node.nags[0];

  return (
    <div className="annotation-panel">
      <div className="nag-picker">
        {NAG_CHOICES.map((n) => (
          <button
            key={n.code}
            title={n.label}
            className={"nag-button" + (activeNag === n.code ? " nag-button--active" : "")}
            onClick={() => onToggleNag(n.code)}
          >
            {n.symbol}
          </button>
        ))}
      </div>
      <label className="annotation-label">
        Commentaire avant le coup
        <textarea value={before} onChange={(e) => setBefore(e.currentTarget.value)} rows={2} />
      </label>
      <label className="annotation-label">
        Commentaire après le coup
        <textarea value={after} onChange={(e) => setAfter(e.currentTarget.value)} rows={2} />
      </label>
    </div>
  );
}
