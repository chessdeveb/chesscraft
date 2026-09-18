import { createContext, useContext } from "react";
import type { Dests, Key } from "@lichess-org/chessground/types";
import type {
  EcoInfo,
  EngineInfo,
  ExplorerFilterValues,
  ExplorerGameSummary,
  ExplorerGamesSort,
  ExplorerGamesSortColumn,
  GameReportProgress,
  GameReportSummary,
  NodeDto,
  OpeningMoveStat,
  ShapeDto,
  TreeDto,
} from "./types";

export interface AppContextValue {
  tree: TreeDto | null;
  currentNode: NodeDto | null;
  currentNodeId: number;
  dests: Dests;
  boardTheme: string;
  setBoardTheme: (theme: string) => void;
  boardFlipped: boolean;
  setBoardFlipped: (flipped: boolean) => void;
  handleMove: (from: Key, to: Key) => void;
  handleShapesChange: (shapes: ShapeDto[]) => void;
  handleSelect: (id: number) => void;
  onStep: (direction: "start" | "prev" | "next" | "end") => void;
  variationChoice: NodeDto[] | null;
  highlightedIndex: number;
  chooseVariation: (id: number) => void;
  handleSetComment: (before: string, after: string) => void;
  handleToggleNag: (code: number) => void;
  handlePromoteVariation: () => void;
  handleDeleteVariation: () => void;
  handleNewChess960: (positionNumber: number) => void;
  explorerStats: OpeningMoveStat[];
  explorerStatsLoading: boolean;
  explorerError: string | null;
  checkSuffixesCleanupCount: number | null;
  ecoInfo: EcoInfo | null;
  explorerFilters: ExplorerFilterValues;
  onExplorerFiltersChange: (filters: ExplorerFilterValues) => void;
  onExplorerFiltersCommit: () => void;
  handleExplorerMove: (san: string) => void;

  explorerGames: ExplorerGameSummary[];
  explorerGamesTotal: number;
  explorerGamesPage: number;
  explorerGamesSort: ExplorerGamesSort | null;
  onExplorerGamesSortChange: (column: ExplorerGamesSortColumn) => void;
  explorerGamesLoading: boolean;
  explorerGamesError: string | null;
  onExplorerGamesPageChange: (page: number) => void;

  previewGameId: number | null;
  previewTree: TreeDto | null;
  previewNodeId: number | null;
  previewError: string | null;
  onSelectPreviewGame: (id: number) => void;
  onSelectPreviewNode: (id: number) => void;
  onPreviewStep: (direction: "start" | "prev" | "next" | "end") => void;
  onOpenPreviewGame: () => void;
  onClosePreview: () => void;

  enginePath: string;
  setEnginePath: (path: string) => void;
  engineRunning: boolean;
  engineStarting: boolean;
  enginePaused: boolean;
  engineDepth: number;
  setEngineDepth: (depth: number) => void;
  engineMultiPv: number;
  setEngineMultiPv: (multiPv: number) => void;
  engineThreads: number;
  setEngineThreads: (threads: number) => void;
  maxThreads: number;
  engineLines: EngineInfo[];
  engineError: string | null;
  handleStartEngine: () => void;
  handleStopEngine: () => void;
  handlePauseEngine: () => void;
  handleResumeEngine: () => void;

  gameReportRunning: boolean;
  gameReportProgress: GameReportProgress | null;
  gameReportError: string | null;
  gameReportSummary: GameReportSummary | null;
  handleStartGameReport: (depth: number, threads: number) => void;
}

export const AppContext = createContext<AppContextValue | null>(null);

export function useAppContext(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) {
    throw new Error("useAppContext doit etre utilise a l'interieur de AppContext.Provider");
  }
  return ctx;
}
