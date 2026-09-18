import { useState } from "react";
import type { IDockviewPanelProps } from "dockview-react";
import type { Color, Key } from "@lichess-org/chessground/types";
import { useTranslation } from "react-i18next";
import { useAppContext } from "./AppContext";
import { Board } from "./components/Board";
import { EnginePanel } from "./components/EnginePanel";
import { ExplorerFiltersBar } from "./components/ExplorerFiltersBar";
import { ExplorerGamesList } from "./components/ExplorerGamesList";
import { ExplorerPreview } from "./components/ExplorerPreview";
import { GameTypeButton } from "./components/GameTypeButton";
import { Notation } from "./components/Notation";
import { OpeningExplorer } from "./components/OpeningExplorer";
import type { TreeDto } from "./types";

const EXPLORER_GAMES_PAGE_SIZE = 20;

const CLOCK_RE = /\[%clk\s+([\d:.]+)\]/;

function clockFromComment(comment: string | null): string | null {
  if (!comment) return null;
  const match = comment.match(CLOCK_RE);
  return match ? match[1] : null;
}

/** Vrai si au moins un coup de la partie porte une annotation d'horloge --
    les pendules ne s'affichent que dans ce cas, pour ne pas ajouter de vide
    a cote de l'echiquier sur l'immense majorite des parties qui n'en ont
    pas. */
function hasAnyClock(tree: TreeDto): boolean {
  return tree.nodes.some((n) => clockFromComment(n.commentAfter) !== null || clockFromComment(n.commentBefore) !== null);
}

/** Dernier temps connu de chaque camp a la position courante : remonte
    l'arbre depuis le noeud affiche jusqu'a la racine, et retient la
    premiere horloge rencontree pour chaque couleur (le coup qui vient de
    l'y amener, ou le plus recent avant lui si ce coup precis n'en porte
    pas). Les Blancs jouent aux profondeurs impaires, les Noirs aux paires
    (voir Notation.tsx, meme convention). */
function clocksAtNode(tree: TreeDto, nodeId: number): { white: string | null; black: string | null } {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));

  let depth = 0;
  for (let n = byId.get(nodeId); n && n.parent !== null; n = byId.get(n.parent)) {
    depth += 1;
  }

  let white: string | null = null;
  let black: string | null = null;
  let node = byId.get(nodeId);
  let d = depth;
  while (node && node.parent !== null && (white === null || black === null)) {
    const clock = clockFromComment(node.commentAfter);
    if (clock) {
      if (d % 2 === 1 && white === null) white = clock;
      else if (d % 2 === 0 && black === null) black = clock;
    }
    node = byId.get(node.parent);
    d -= 1;
  }
  return { white, black };
}

export function BoardPanel() {
  const { t } = useTranslation();
  const {
    tree,
    currentNode,
    currentNodeId,
    dests,
    boardTheme,
    boardFlipped,
    setBoardFlipped,
    handleMove,
    handleShapesChange,
    onStep,
    handleNewChess960,
  } = useAppContext();
  const [fenCopied, setFenCopied] = useState(false);
  // largeur reelle (px) de l'echiquier, remontee par Board via onSizeChange :
  // l'echiquier est un carre centre dans .board-panel-board (qui, lui, fait
  // toute la largeur du panneau), donc s'aligner sur la largeur du PANNEAU
  // (comme un simple flex justify-content:flex-end) deborde du bord reel de
  // l'echiquier des que le panneau est plus large que haut.
  const [boardSize, setBoardSize] = useState(480);
  if (!currentNode) return null;
  const lastMove: [Key, Key] | null =
    currentNode.from && currentNode.to ? [currentNode.from as Key, currentNode.to as Key] : null;

  async function handleCopyFen() {
    try {
      await navigator.clipboard.writeText(currentNode!.fen);
      setFenCopied(true);
      setTimeout(() => setFenCopied(false), 1200);
    } catch {
      // presse-papiers indisponible (rare) : pas d'action bloquante pour si peu.
    }
  }

  const showClocks = tree ? hasAnyClock(tree) : false;
  const clocks = showClocks && tree ? clocksAtNode(tree, currentNodeId) : { white: null, black: null };
  const clockRowStyle = { width: boardSize, marginLeft: "auto", marginRight: "auto" };
  // en haut = camp affiche en haut de l'echiquier : Noirs normalement,
  // Blancs une fois l'echiquier retourne.
  const topClock = boardFlipped ? clocks.white : clocks.black;
  const bottomClock = boardFlipped ? clocks.black : clocks.white;

  return (
    <div className="board-panel">
      {showClocks && (
        <div className="board-clock-row" style={clockRowStyle}>
          {topClock && <span className="board-clock">🕐{topClock}</span>}
        </div>
      )}
      <div className="board-panel-board">
        <Board
          fen={currentNode.fen}
          turnColor={currentNode.turn as Color}
          dests={dests}
          shapes={currentNode.shapes}
          theme={boardTheme}
          lastMove={lastMove}
          orientation={boardFlipped ? "black" : "white"}
          onMove={handleMove}
          onShapesChange={handleShapesChange}
          onSizeChange={setBoardSize}
        />
      </div>
      {showClocks && (
        <div className="board-clock-row" style={clockRowStyle}>
          {bottomClock && <span className="board-clock">🕐{bottomClock}</span>}
        </div>
      )}
      <div className="transport">
        <button onClick={() => setBoardFlipped(!boardFlipped)} title={t("board.flip")}>
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
        <button onClick={handleCopyFen} title={t("board.copyFen")}>
          {fenCopied ? "✓" : "📋"}
        </button>
        <GameTypeButton onStartChess960={handleNewChess960} />
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
    handlePromoteVariation,
    handleDeleteVariation,
    maxThreads,
    gameReportRunning,
    gameReportProgress,
    gameReportError,
    gameReportSummary,
    handleStartGameReport,
    boardTheme,
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
        onPromoteVariation={handlePromoteVariation}
        onDeleteVariation={handleDeleteVariation}
        maxThreads={maxThreads}
        onStartGameReport={handleStartGameReport}
        gameReportRunning={gameReportRunning}
        gameReportProgress={gameReportProgress}
        gameReportError={gameReportError}
        gameReportSummary={gameReportSummary}
        boardTheme={boardTheme}
      />
    </div>
  );
}

export function ExplorerPanelView() {
  const { t } = useTranslation();
  const [showFilters, setShowFilters] = useState(false);
  const {
    explorerStats,
    explorerStatsLoading,
    explorerError,
    checkSuffixesCleanupCount,
    ecoInfo,
    explorerFilters,
    onExplorerFiltersChange,
    onExplorerFiltersCommit,
    handleExplorerMove,
    explorerGames,
    explorerGamesTotal,
    explorerGamesPage,
    explorerGamesSort,
    onExplorerGamesSortChange,
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
    boardFlipped,
  } = useAppContext();
  return (
    <div className="dock-panel-content explorer-panel">
      <div className="explorer-panel-col explorer-panel-col--left">
        {checkSuffixesCleanupCount !== null && (
          <p className="explorer-cleanup-status">{t("explorer.checkSuffixCleanup", { count: checkSuffixesCleanupCount.toLocaleString() })}</p>
        )}
        <button className="explorer-filters-toggle" onClick={() => setShowFilters((v) => !v)}>
          {t("explorer.filters")} {showFilters ? "▲" : "▼"}
        </button>
        {showFilters && <ExplorerFiltersBar filters={explorerFilters} onChange={onExplorerFiltersChange} onCommit={onExplorerFiltersCommit} />}
        <OpeningExplorer stats={explorerStats} loading={explorerStatsLoading} error={explorerError} ecoInfo={ecoInfo} onSelectMove={handleExplorerMove} />
        <ExplorerGamesList
          games={explorerGames}
          total={explorerGamesTotal}
          page={explorerGamesPage}
          pageSize={EXPLORER_GAMES_PAGE_SIZE}
          sort={explorerGamesSort}
          onSortChange={onExplorerGamesSortChange}
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
          initialFlipped={boardFlipped}
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
    currentNode,
    boardTheme,
    boardFlipped,
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
        analyzedFen={currentNode?.fen ?? null}
        boardTheme={boardTheme}
        orientation={boardFlipped ? "black" : "white"}
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
