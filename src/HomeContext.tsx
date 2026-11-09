import { createContext, useContext } from "react";
import type { BaseInfo, CatchupRange, FidePlayer, GameSummary, PgnGameSummary, PlayerSummary, TournamentSummary } from "./types";

export type HomeActiveSource = { kind: "base"; base: BaseInfo } | { kind: "pgn"; pgn: BaseInfo } | null;

export type GamesSortColumn = "white" | "black" | "date";
export interface GamesSort {
  column: GamesSortColumn;
  dir: "asc" | "desc";
}

export type PlayersSortColumn = "name" | "games";
export interface PlayersSort {
  column: PlayersSortColumn;
  dir: "asc" | "desc";
}

export type FideSortColumn =
  | "name"
  | "fed"
  | "sex"
  | "title"
  | "wTitle"
  | "oTitle"
  | "std"
  | "rapid"
  | "blitz"
  | "birthYear"
  | "flag";
export interface FideSort {
  column: FideSortColumn;
  dir: "asc" | "desc";
}

export type TournamentsSortColumn = "name" | "site" | "timeControl" | "dates" | "games";
export interface TournamentsSort {
  column: TournamentsSortColumn;
  dir: "asc" | "desc";
}

export interface GamesAdvancedFilter {
  white: string;
  black: string;
  result: string;
  dateFrom: string;
  dateTo: string;
  tournament: string;
}

export const EMPTY_GAMES_ADVANCED_FILTER: GamesAdvancedFilter = {
  white: "",
  black: "",
  result: "",
  dateFrom: "",
  dateTo: "",
  tournament: "",
};

export interface FideAdvancedFilter {
  fed: string;
  title: string;
  sex: string;
  status: string;
  birthYearMin: string;
  birthYearMax: string;
}

export const EMPTY_FIDE_ADVANCED_FILTER: FideAdvancedFilter = {
  fed: "",
  title: "",
  sex: "",
  status: "",
  birthYearMin: "",
  birthYearMax: "",
};

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

  externalUpdatesEstimate: CatchupRange | null;
  externalUpdatesBusy: "estimating" | "checking" | null;
  onCheckExternalUpdates: () => void;

  games: GameSummary[];
  gamesTotal: number;
  gamesLoading: boolean;
  gamesPage: number;
  gamesSearchInput: string;
  onGamesSearchChange: (v: string) => void;
  onGamesPageChange: (p: number) => void;
  gamesSort: GamesSort | null;
  onGamesSortChange: (column: GamesSortColumn) => void;
  gamesAdvancedOpen: boolean;
  onToggleGamesAdvanced: () => void;
  gamesAdvancedFilter: GamesAdvancedFilter;
  onGamesAdvancedFilterChange: (patch: Partial<GamesAdvancedFilter>) => void;
  onGamesFilterCommit: () => void;
  onOpenBaseGame: (id: number) => void;
  onDeleteGame: (id: number) => void;
  onDeleteGames: (ids: number[]) => void;
  onMoveGame: (id: number) => void;
  onTransferGames: (ids: number[], mode: "move" | "copy") => void;
  onDropGamesOnBase: (base: BaseInfo, ids: number[], fromPath: string, ctrlKey: boolean) => void;
  selectedGameIds: Set<number>;
  onGamesSelectionChange: (ids: Set<number>) => void;

  pgnGames: PgnGameSummary[];
  onOpenPgnGame: (index: number) => void;

  players: PlayerSummary[];
  playersTotal: number;
  playersLoading: boolean;
  playersPage: number;
  playersSearchInput: string;
  onPlayersSearchChange: (v: string) => void;
  onPlayersPageChange: (p: number) => void;
  onPlayersFilterCommit: () => void;
  onSelectPlayer: (name: string) => void;
  playersSort: PlayersSort | null;
  onPlayersSortChange: (column: PlayersSortColumn) => void;
  onPrepareAgainstPlayer: (name: string, white: boolean) => void;

  fidePlayers: FidePlayer[];
  fideTotal: number;
  fideLoading: boolean;
  fidePage: number;
  fideSearchInput: string;
  onFideSearchChange: (v: string) => void;
  onFidePageChange: (p: number) => void;
  onFideFilterCommit: () => void;
  fideAdvancedOpen: boolean;
  onToggleFideAdvanced: () => void;
  fideAdvancedFilter: FideAdvancedFilter;
  onFideAdvancedFilterChange: (patch: Partial<FideAdvancedFilter>) => void;
  fideSort: FideSort | null;
  onFideSortChange: (column: FideSortColumn) => void;
  fideLastUpdated: string | null;
  fideRefreshing: boolean;
  fideRefreshProgress: number;
  fideRefreshError: string | null;
  onFideRefresh: () => void;

  tournaments: TournamentSummary[];
  tournamentsTotal: number;
  tournamentsLoading: boolean;
  tournamentsPage: number;
  tournamentsSearchInput: string;
  onTournamentsSearchChange: (v: string) => void;
  onTournamentsPageChange: (p: number) => void;
  onTournamentsFilterCommit: () => void;
  tournamentsSort: TournamentsSort | null;
  onTournamentsSortChange: (column: TournamentsSortColumn) => void;
  onSetTournamentCadence: (tournamentId: number, timeControl: string) => void;
  onFilterGamesByTournament: (name: string) => void;
}

export const HomeContext = createContext<HomeContextValue | null>(null);

export function useHomeContext(): HomeContextValue {
  const ctx = useContext(HomeContext);
  if (!ctx) {
    throw new Error("useHomeContext doit etre utilise a l'interieur de HomeContext.Provider");
  }
  return ctx;
}
