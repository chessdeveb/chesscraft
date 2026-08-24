import { createContext, useContext } from "react";
import type { BaseInfo, GameSummary, PgnGameSummary } from "./types";

export type HomeActiveSource = { kind: "base"; base: BaseInfo } | { kind: "pgn"; pgn: BaseInfo } | null;

export type GamesSortColumn = "white" | "black" | "date";
export interface GamesSort {
  column: GamesSortColumn;
  dir: "asc" | "desc";
}

export interface HomeContextValue {
  error: string | null;

  bases: BaseInfo[];
  onSelectBase: (base: BaseInfo) => void;
  onCreateBase: () => void;
  onAddExistingBase: () => void;

  recentPgns: BaseInfo[];
  onSelectPgn: (pgn: BaseInfo) => void;
  onImportClick: () => void;

  activeSource: HomeActiveSource;
  onToggleReference: () => void;
  onDeleteBase: () => void;

  games: GameSummary[];
  gamesTotal: number;
  gamesPage: number;
  gamesSearchInput: string;
  onGamesSearchChange: (v: string) => void;
  onGamesPageChange: (p: number) => void;
  gamesSort: GamesSort | null;
  onGamesSortChange: (column: GamesSortColumn) => void;
  onOpenBaseGame: (id: number) => void;
  onDeleteGame: (id: number) => void;

  pgnGames: PgnGameSummary[];
  onOpenPgnGame: (index: number) => void;
}

export const HomeContext = createContext<HomeContextValue | null>(null);

export function useHomeContext(): HomeContextValue {
  const ctx = useContext(HomeContext);
  if (!ctx) {
    throw new Error("useHomeContext doit etre utilise a l'interieur de HomeContext.Provider");
  }
  return ctx;
}
