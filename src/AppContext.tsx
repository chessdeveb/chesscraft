import { createContext, useContext } from "react";
import type { Dests, Key } from "@lichess-org/chessground/types";
import type { EngineInfo, NodeDto, OpeningMoveStat, ShapeDto, TreeDto } from "./types";

export interface AppContextValue {
  tree: TreeDto | null;
  currentNode: NodeDto | null;
  currentNodeId: number;
  dests: Dests;
  boardTheme: string;
  setBoardTheme: (theme: string) => void;
  handleMove: (from: Key, to: Key) => void;
  handleShapesChange: (shapes: ShapeDto[]) => void;
  handleSelect: (id: number) => void;
  variationChoice: NodeDto[] | null;
  highlightedIndex: number;
  chooseVariation: (id: number) => void;
  handleSetComment: (before: string, after: string) => void;
  handleToggleNag: (code: number) => void;
  explorerStats: OpeningMoveStat[];
  explorerError: string | null;
  handleExplorerMove: (san: string) => void;

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
}

export const AppContext = createContext<AppContextValue | null>(null);

export function useAppContext(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) {
    throw new Error("useAppContext doit etre utilise a l'interieur de AppContext.Provider");
  }
  return ctx;
}
