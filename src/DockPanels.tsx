import type { IDockviewPanelProps } from "dockview-react";
import type { Color } from "@lichess-org/chessground/types";
import { useAppContext } from "./AppContext";
import { Board } from "./components/Board";
import { EnginePanel } from "./components/EnginePanel";
import { Notation } from "./components/Notation";
import { OpeningExplorer } from "./components/OpeningExplorer";

export function BoardPanel() {
  const { currentNode, dests, boardTheme, handleMove, handleShapesChange } = useAppContext();
  if (!currentNode) return null;
  return (
    <Board
      fen={currentNode.fen}
      turnColor={currentNode.turn as Color}
      dests={dests}
      shapes={currentNode.shapes}
      theme={boardTheme}
      onMove={handleMove}
      onShapesChange={handleShapesChange}
    />
  );
}

export function NotationPanelView() {
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
          <p>Continuer avec :</p>
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
  const { explorerStats, explorerError, handleExplorerMove } = useAppContext();
  return (
    <div className="dock-panel-content">
      <OpeningExplorer stats={explorerStats} error={explorerError} onSelectMove={handleExplorerMove} />
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
