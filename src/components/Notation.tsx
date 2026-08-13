import type { ReactNode } from "react";
import { nagSymbol } from "../nag";
import type { NodeDto, TreeDto } from "../types";

interface NotationProps {
  tree: TreeDto;
  currentNodeId: number;
  onSelect: (id: number) => void;
}

export function Notation({ tree, currentNodeId, onSelect }: NotationProps) {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));

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
    return (
      <span key={node.id}>
        {label && <span className="move-number">{label} </span>}
        {node.commentBefore && <span className="comment">{node.commentBefore} </span>}
        <span
          className={"move" + (node.id === currentNodeId ? " move--current" : "")}
          onClick={() => onSelect(node.id)}
        >
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

  return (
    <div className="notation">
      {firstMoveId !== undefined ? (
        renderLine(firstMoveId, true)
      ) : (
        <p className="notation-empty">Aucun coup joué</p>
      )}
    </div>
  );
}
