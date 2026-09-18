import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";
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
  /** Orientation de depart de CET echiquier -- meme sens que l'echiquier
      principal au premier affichage, mais ensuite retournable independamment
      (voir le bouton 🔄 ci-dessous) : cet apercu montre souvent une AUTRE
      partie que celle en cours sur l'echiquier principal, retourner l'un ne
      doit pas affecter l'autre. */
  initialFlipped: boolean;
}

export function ExplorerPreview({ gameId, tree, nodeId, error, onSelectNode, onStep, onOpen, onClose, boardTheme, initialFlipped }: ExplorerPreviewProps) {
  const { t } = useTranslation();
  const [flipped, setFlipped] = useState(initialFlipped);

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
              lastMove={currentNode.from && currentNode.to ? [currentNode.from as Key, currentNode.to as Key] : null}
              orientation={flipped ? "black" : "white"}
              onMove={NO_OP}
              onShapesChange={NO_OP}
            />
          </div>
          <div className="explorer-preview-controls">
            <button onClick={() => setFlipped((v) => !v)} title={t("board.flip")}>
              🔄
            </button>
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
              {t("explorer.previewOpenShort")}
            </button>
            <button onClick={onClose} title={t("notation.close")}>
              ✕
            </button>
          </div>
          <div className="explorer-preview-notation">
            <Notation tree={tree} currentNodeId={nodeId ?? tree.root} onSelect={onSelectNode} readOnly boardTheme={boardTheme} />
          </div>
        </>
      )}
    </div>
  );
}
