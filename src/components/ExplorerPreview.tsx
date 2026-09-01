import { useTranslation } from "react-i18next";
import type { Color, Dests } from "@lichess-org/chessground/types";
import { Board } from "./Board";
import { Notation } from "./Notation";
import type { TreeDto } from "../types";

const NO_DESTS: Dests = new Map();
const NO_OP = () => {};

interface ExplorerPreviewProps {
  gameId: number | null;
  tree: TreeDto | null;
  nodeId: number | null;
  error: string | null;
  onSelectNode: (id: number) => void;
  onStep: (direction: "start" | "prev" | "next" | "end") => void;
  onOpen: () => void;
  onClose: () => void;
  boardTheme: string;
}

export function ExplorerPreview({ gameId, tree, nodeId, error, onSelectNode, onStep, onOpen, onClose, boardTheme }: ExplorerPreviewProps) {
  const { t } = useTranslation();

  if (gameId === null) {
    return (
      <div className="explorer-preview explorer-preview--empty">
        <p className="notation-empty">{t("explorer.previewEmpty")}</p>
      </div>
    );
  }

  const currentNode = tree?.nodes.find((n) => n.id === nodeId) ?? null;

  return (
    <div className="explorer-preview">
      {error && <p className="error">{error}</p>}
      {!currentNode && !error && <p className="notation-empty">{t("common.loading")}</p>}
      {currentNode && tree && (
        <>
          <div className="explorer-preview-board">
            <Board
              fen={currentNode.fen}
              turnColor={currentNode.turn as Color}
              dests={NO_DESTS}
              shapes={[]}
              theme={boardTheme}
              onMove={NO_OP}
              onShapesChange={NO_OP}
            />
          </div>
          <div className="explorer-preview-controls">
            <button onClick={() => onStep("start")} title={t("explorer.previewStart")}>
              ⏮
            </button>
            <button onClick={() => onStep("prev")} title={t("explorer.previewPrev")}>
              ◀
            </button>
            <button onClick={() => onStep("next")} title={t("explorer.previewNext")}>
              ▶
            </button>
            <button onClick={() => onStep("end")} title={t("explorer.previewEnd")}>
              ⏭
            </button>
            <button onClick={onOpen} title={t("explorer.previewOpen")}>
              {t("explorer.previewOpen")}
            </button>
            <button onClick={onClose} title={t("notation.close")}>
              ✕
            </button>
          </div>
          <div className="explorer-preview-notation">
            <Notation tree={tree} currentNodeId={nodeId ?? tree.root} onSelect={onSelectNode} onSetComment={NO_OP} onToggleNag={NO_OP} />
          </div>
        </>
      )}
    </div>
  );
}
