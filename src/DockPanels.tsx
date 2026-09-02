import type { IDockviewPanelProps } from "dockview-react";
import type { Color, Key } from "@lichess-org/chessground/types";
import { useTranslation } from "react-i18next";
import { useAppContext } from "./AppContext";
import { Board } from "./components/Board";
import { EnginePanel } from "./components/EnginePanel";
import { ExplorerGamesList } from "./components/ExplorerGamesList";
import { ExplorerPreview } from "./components/ExplorerPreview";
import { Notation } from "./components/Notation";
import { OpeningExplorer } from "./components/OpeningExplorer";

const EXPLORER_GAMES_PAGE_SIZE = 20;

export function BoardPanel() {
  const { t } = useTranslation();
  const { currentNode, dests, boardTheme, handleMove, handleShapesChange, onStep } = useAppContext();
  if (!currentNode) return null;
  const lastMove: [Key, Key] | null =
    currentNode.from && currentNode.to ? [currentNode.from as Key, currentNode.to as Key] : null;
  return (
    <div className="board-panel">
      <div className="board-panel-board">
        <Board
          fen={currentNode.fen}
          turnColor={currentNode.turn as Color}
          dests={dests}
          shapes={currentNode.shapes}
          theme={boardTheme}
          lastMove={lastMove}
          onMove={handleMove}
          onShapesChange={handleShapesChange}
        />
      </div>
      <div className="transport">
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
      </div>
    </div>
  );
}

export function NotationPanelView() {
  const { t } = useTranslation();
  const {
    tree,
    currentNodeId,
    handleSelect,
    variationChoice,
    highlightedIndex,
    chooseVariation,
    handleSetComment,
    handleToggleNag,
  } = useAppContext();
  if (!tree) return null;
  return (
    <div className="dock-panel-content">
      {variationChoice && (
        <div className="variation-choice">
          <p>{t("dockPanels.continueWith")}</p>
          <div className="variation-choice-options">
            {variationChoice.map((option, index) => (
              <button
                key={option.id}
                className={index === highlightedIndex ? "variation-choice-highlighted" : ""}
                onClick={() => chooseVariation(option.id)}
              >
                {option.san}
              </button>
            ))}
          </div>
        </div>
      )}
      <Notation
        tree={tree}
        currentNodeId={currentNodeId}
        onSelect={handleSelect}
        onSetComment={handleSetComment}
        onToggleNag={handleToggleNag}
      />
    </div>
  );
}

export function ExplorerPanelView() {
  const {
    explorerStats,
    explorerError,
    handleExplorerMove,
    explorerGames,
    explorerGamesTotal,
    explorerGamesPage,
    explorerGamesLoading,
    explorerGamesError,
    onExplorerGamesPageChange,
    previewGameId,
    previewTree,
    previewNodeId,
    previewError,
    onSelectPreviewGame,
    onSelectPreviewNode,
    onPreviewStep,
    onOpenPreviewGame,
    onClosePreview,
    boardTheme,
  } = useAppContext();
  return (
    <div className="dock-panel-content explorer-panel">
      <div className="explorer-panel-col explorer-panel-col--left">
        <OpeningExplorer stats={explorerStats} error={explorerError} onSelectMove={handleExplorerMove} />
        <ExplorerGamesList
          games={explorerGames}
          total={explorerGamesTotal}
          page={explorerGamesPage}
          pageSize={EXPLORER_GAMES_PAGE_SIZE}
          loading={explorerGamesLoading}
          error={explorerGamesError}
          onPageChange={onExplorerGamesPageChange}
          selectedGameId={previewGameId}
          onSelectGame={onSelectPreviewGame}
        />
      </div>
      <div className="explorer-panel-col explorer-panel-col--right">
        <ExplorerPreview
          gameId={previewGameId}
          tree={previewTree}
          nodeId={previewNodeId}
          error={previewError}
          onSelectNode={onSelectPreviewNode}
          onStep={onPreviewStep}
          onOpen={onOpenPreviewGame}
          onClose={onClosePreview}
          boardTheme={boardTheme}
        />
      </div>
    </div>
  );
}

export function EnginePanelView(props: IDockviewPanelProps) {
  const {
    enginePath,
    setEnginePath,
    engineRunning,
    engineStarting,
    enginePaused,
    engineDepth,
    setEngineDepth,
    engineMultiPv,
    setEngineMultiPv,
    engineThreads,
    setEngineThreads,
    maxThreads,
    engineLines,
    engineError,
    handleStartEngine,
    handleStopEngine,
    handlePauseEngine,
    handleResumeEngine,
  } = useAppContext();

  return (
    <div className="dock-panel-content">
      <EnginePanel
        enginePath={enginePath}
        onEnginePathChange={setEnginePath}
        running={engineRunning}
        starting={engineStarting}
        paused={enginePaused}
        depth={engineDepth}
        onDepthChange={setEngineDepth}
        multiPv={engineMultiPv}
        onMultiPvChange={setEngineMultiPv}
        threads={engineThreads}
        onThreadsChange={setEngineThreads}
        maxThreads={maxThreads}
        lines={engineLines}
        error={engineError}
        onStart={handleStartEngine}
        onStop={handleStopEngine}
        onPause={handlePauseEngine}
        onResume={handleResumeEngine}
        onClose={() => {
          handleStopEngine();
          props.api.close();
        }}
      />
    </div>
  );
}

export const dockviewComponents = {
  board: BoardPanel,
  notation: NotationPanelView,
  explorer: ExplorerPanelView,
  engine: EnginePanelView,
};
