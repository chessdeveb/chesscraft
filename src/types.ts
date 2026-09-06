export interface MoveDto {
  from: string;
  to: string;
  promotion: string | null;
  san: string;
}

export interface ShapeDto {
  orig: string;
  dest: string | null;
  brush: string;
}

export interface NodeDto {
  id: number;
  parent: number | null;
  children: number[];
  san: string | null;
  from: string | null;
  to: string | null;
  fen: string;
  turn: "white" | "black";
  commentBefore: string | null;
  commentAfter: string | null;
  nags: number[];
  shapes: ShapeDto[];
}

export interface TreeDto {
  nodes: NodeDto[];
  root: number;
  tags: Record<string, string>;
}

export interface PvMove {
  san: string;
  fen: string;
  from: string;
  to: string;
}

export interface EngineInfo {
  depth: number;
  multipv: number;
  scoreCp: number | null;
  scoreMate: number | null;
  pv: PvMove[];
}

export interface PgnGameSummary {
  index: number;
  white: string;
  black: string;
  date: string;
  result: string;
  event: string;
}

export interface OpeningMoveStat {
  san: string;
  games: number;
  whiteWins: number;
  draws: number;
  blackWins: number;
}

export interface EcoInfo {
  eco: string;
  name: string;
}

export interface ExplorerFilterValues {
  playerName: string;
  playerWhite: boolean;
  ratingMin: string;
  ratingMax: string;
  yearMin: string;
  yearMax: string;
}

export interface BaseInfo {
  name: string;
  path: string;
  games: number;
  isReference: boolean;
  missing: boolean;
}

export interface GameSummary {
  id: number;
  white: string;
  black: string;
  date: string;
  result: string;
  event: string;
  timeControl: string;
}

export type SaveResult = { kind: "saved"; id: number } | { kind: "duplicate"; existing: GameSummary };

export interface ExplorerGameSummary extends GameSummary {
  whiteElo: number | null;
  blackElo: number | null;
}

export type ExplorerGamesSortColumn = "white" | "black" | "date" | "whiteElo" | "blackElo";
export interface ExplorerGamesSort {
  column: ExplorerGamesSortColumn;
  dir: "asc" | "desc";
}

export interface ExplorerGamesPage {
  games: ExplorerGameSummary[];
  total: number;
  basePath: string;
}

export interface PlayerSummary {
  name: string;
  games: number;
}

export interface PlayersPage {
  players: PlayerSummary[];
  total: number;
}

export interface FidePlayer {
  fideId: string;
  name: string;
  fed: string;
  sex: string;
  title: string;
  wTitle: string;
  oTitle: string;
  stdRating: number | null;
  rapidRating: number | null;
  blitzRating: number | null;
  birthYear: number | null;
  flag: string;
}

export interface FidePlayersPage {
  players: FidePlayer[];
  total: number;
}

export interface FideStatus {
  lastUpdated: string | null;
  total: number;
}

export interface TabDto {
  tabId: number;
  tree: TreeDto;
  isPlayerPrep: boolean;
}

export interface BulkImportSummary {
  total: number;
  imported: number;
  skippedDuplicates: number;
  skippedInvalid: number;
  failed: number;
}
